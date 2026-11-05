use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

#[cfg(target_os = "linux")]
mod discovery;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    pub port: String,
    pub baud: u32,
    pub data: DataBits,
    pub stop: StopBits,
    pub parity: Parity,
    pub flow: FlowControl,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud: 115_200,
            data: DataBits::Eight,
            stop: StopBits::One,
            parity: Parity::None,
            flow: FlowControl::None,
        }
    }
}

impl Settings {
    pub fn summary(&self) -> String {
        let parity = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        let flow = match self.flow {
            FlowControl::None => "",
            FlowControl::Hardware => " · RTS/CTS",
            FlowControl::Software => " · XON/XOFF",
        };
        format!(
            "{} · {} · {}{parity}{}{flow}",
            self.port,
            self.baud,
            data_bits_number(self.data),
            stop_bits_number(self.stop)
        )
    }
}

pub fn data_bits_number(bits: DataBits) -> u8 {
    match bits {
        DataBits::Five => 5,
        DataBits::Six => 6,
        DataBits::Seven => 7,
        DataBits::Eight => 8,
    }
}

pub fn stop_bits_number(bits: StopBits) -> u8 {
    match bits {
        StopBits::One => 1,
        StopBits::Two => 2,
    }
}

pub enum Command {
    Scan,
    Open(Settings),
    Close,
    Write {
        data: Vec<u8>,
        session_epoch: usize,
        repeat_epoch: Option<usize>,
    },
}

pub enum Event {
    Ports(Vec<String>),
    Opened(Settings),
    Closed,
    OpenFailed {
        error: String,
        retained: Option<Settings>,
        restore_error: Option<String>,
    },
    ScanFailed(String),
    WriteFailed(String),
    Lost(String),
    Rx(Vec<u8>),
    Tx(Vec<u8>),
}

#[derive(Default)]
struct Control {
    stop: AtomicBool,
    discard_rx: AtomicBool,
    tx_epoch: AtomicUsize,
    repeat_epoch: AtomicUsize,
}

impl Control {
    fn allows_write(&self, session_epoch: usize, repeat_epoch: Option<usize>) -> bool {
        session_epoch == self.tx_epoch.load(SeqCst)
            && repeat_epoch.is_none_or(|epoch| epoch == self.repeat_epoch.load(SeqCst))
    }
}

pub struct Worker {
    commands: SyncSender<Command>,
    pub events: Option<Receiver<Event>>,
    control: Arc<Control>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start() -> io::Result<Self> {
        let (commands, command_rx) = mpsc::sync_channel(24);
        let (event_tx, events) = mpsc::sync_channel(128);
        let control = Arc::new(Control::default());
        let thread_control = Arc::clone(&control);
        let thread = thread::Builder::new()
            .name("serial-io".into())
            .spawn(move || {
                let _ = run(command_rx, event_tx, &thread_control);
            })?;
        Ok(Self {
            commands,
            events: Some(events),
            control,
            thread: Some(thread),
        })
    }

    pub fn enqueue(&self, command: Command) -> Result<(), &'static str> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => "busy",
                TrySendError::Disconnected(_) => "worker_down",
            })
    }

    pub fn write(&self, data: Vec<u8>, repeat: bool) -> Result<(), &'static str> {
        self.enqueue(Command::Write {
            data,
            session_epoch: self.control.tx_epoch.load(SeqCst),
            repeat_epoch: repeat.then(|| self.control.repeat_epoch.load(SeqCst)),
        })
    }

    pub fn invalidate_writes(&self) {
        self.control.tx_epoch.fetch_add(1, SeqCst);
    }

    pub fn cancel_repeat_writes(&self) {
        self.control.repeat_epoch.fetch_add(1, SeqCst);
    }

    pub fn discard_rx(&self, discard: bool) {
        self.control.discard_rx.store(discard, SeqCst);
    }

    pub fn stop(&self) {
        self.control.stop.store(true, SeqCst);
        self.invalidate_writes();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
        // A worker waiting for log queue space must be able to exit before join.
        drop(self.events.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn open(settings: &Settings) -> serialport::Result<Box<dyn SerialPort>> {
    serialport::new(&settings.port, settings.baud)
        .data_bits(settings.data)
        .stop_bits(settings.stop)
        .parity(settings.parity)
        .flow_control(settings.flow)
        .timeout(Duration::from_millis(15))
        .open()
}

struct Connection {
    port: Box<dyn SerialPort>,
    settings: Settings,
}

pub fn same_device(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    #[cfg(target_os = "linux")]
    return discovery::device_id(left.as_ref())
        .zip(discovery::device_id(right.as_ref()))
        .is_some_and(|(left, right)| left == right);
    #[cfg(not(target_os = "linux"))]
    std::fs::canonicalize(left)
        .ok()
        .zip(std::fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

fn change_connection(connection: &mut Option<Connection>, settings: Settings) -> Event {
    // Exclusive opens prevent opening the same device twice, even through a symlink.
    let same = connection
        .as_ref()
        .is_some_and(|current| same_device(&current.settings.port, &settings.port));
    let rollback = if same {
        connection.take().map(|previous| previous.settings)
    } else {
        None
    };
    match open(&settings) {
        Ok(port) => {
            *connection = Some(Connection {
                port,
                settings: settings.clone(),
            });
            Event::Opened(settings)
        }
        Err(error) => {
            let mut restore_error = None;
            if let Some(previous) = rollback {
                match open(&previous) {
                    Ok(port) => {
                        *connection = Some(Connection {
                            port,
                            settings: previous,
                        });
                    }
                    Err(error) => restore_error = Some(error.to_string()),
                }
            }
            Event::OpenFailed {
                error: error.to_string(),
                retained: connection.as_ref().map(|current| current.settings.clone()),
                restore_error,
            }
        }
    }
}

fn scan() -> serialport::Result<Vec<String>> {
    let ports: Vec<String> = serialport::available_ports()?
        .into_iter()
        .map(|port| port.port_name)
        .collect();
    #[cfg(target_os = "linux")]
    let ports = discovery::scan(ports);
    let mut ports = ports;
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn run(
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    control: &Control,
) -> Result<(), mpsc::SendError<Event>> {
    let mut connection = None;
    let mut buf = [0; 4096];
    while !control.stop.load(SeqCst) {
        let command = if connection.is_some() {
            commands.try_recv().ok()
        } else {
            commands.recv_timeout(Duration::from_millis(15)).ok()
        };
        match command {
            Some(Command::Scan) => match scan() {
                Ok(ports) => events.send(Event::Ports(ports))?,
                Err(error) => events.send(Event::ScanFailed(error.to_string()))?,
            },
            Some(Command::Open(settings)) => {
                events.send(change_connection(&mut connection, settings))?;
            }
            Some(Command::Close) => {
                drop(connection.take());
                events.send(Event::Closed)?;
            }
            Some(Command::Write {
                data,
                session_epoch,
                repeat_epoch,
            }) => {
                if let Some(current) = &mut connection {
                    let (count, error) = write_packet(
                        current.port.as_mut(),
                        &data,
                        session_epoch,
                        repeat_epoch,
                        control,
                    );
                    if count > 0 {
                        events.send(Event::Tx(data[..count].to_vec()))?;
                    }
                    if let Some(error) = error {
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        ) {
                            events.send(Event::WriteFailed(error.to_string()))?;
                        } else {
                            drop(connection.take());
                            events.send(Event::Lost(error.to_string()))?;
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(current) = &mut connection {
            match current.port.read(&mut buf) {
                Ok(0) => {
                    drop(connection.take());
                    events.send(Event::Lost("end of stream".into()))?;
                }
                Ok(size) if !control.discard_rx.load(SeqCst) => {
                    events.send(Event::Rx(buf[..size].to_vec()))?;
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    drop(connection.take());
                    events.send(Event::Lost(error.to_string()))?;
                }
            }
        }
    }
    Ok(())
}

fn write_packet(
    port: &mut (impl io::Write + ?Sized),
    data: &[u8],
    session_epoch: usize,
    repeat_epoch: Option<usize>,
    control: &Control,
) -> (usize, Option<io::Error>) {
    let mut count = 0;
    while count < data.len() {
        // Finish a frame that has already started unless the serial session changes.
        let queued_repeat = if count == 0 { repeat_epoch } else { None };
        if !control.allows_write(session_epoch, queued_repeat) {
            break;
        }
        match port.write(&data[count..]) {
            Ok(0) => return (count, Some(io::ErrorKind::WriteZero.into())),
            Ok(size) => count += size,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return (count, Some(error)),
        }
    }
    (count, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_repeat_skips_queued_repeat_frames_but_keeps_manual_sends() {
        let (commands, queued) = mpsc::sync_channel(4);
        let worker = Worker {
            commands,
            events: None,
            control: Arc::new(Control::default()),
            thread: None,
        };
        worker.write(b"old repeat".to_vec(), true).unwrap();
        worker.write(b"manual".to_vec(), false).unwrap();
        worker.cancel_repeat_writes();
        worker.write(b"new repeat".to_vec(), true).unwrap();

        let mut device = Vec::new();
        for _ in 0..3 {
            let Command::Write {
                data,
                session_epoch,
                repeat_epoch,
            } = queued.try_recv().unwrap()
            else {
                panic!("expected a write command");
            };
            let (_, error) = write_packet(
                &mut device,
                &data,
                session_epoch,
                repeat_epoch,
                &worker.control,
            );
            assert!(error.is_none());
        }
        assert_eq!(device, b"manualnew repeat");
    }
}
