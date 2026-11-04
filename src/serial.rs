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

use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

#[derive(Clone, Debug, Eq, PartialEq)]
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
    Write(Vec<u8>, usize),
}

pub enum Event {
    Ports(Vec<String>),
    Opened(Settings),
    Closed,
    OpenFailed(String),
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

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), &'static str> {
        self.enqueue(Command::Write(bytes, self.control.tx_epoch.load(SeqCst)))
    }

    pub fn invalidate_writes(&self) {
        self.control.tx_epoch.fetch_add(1, SeqCst);
    }

    pub fn discard_rx(&self, discard: bool) {
        self.control.discard_rx.store(discard, SeqCst);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.control.stop.store(true, SeqCst);
        self.invalidate_writes();
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

fn scan() -> serialport::Result<Vec<String>> {
    let mut ports: Vec<_> = serialport::available_ports()?
        .into_iter()
        .map(|port| port.port_name)
        .collect();
    if let Ok(entries) = std::fs::read_dir("/dev/serial/by-id") {
        ports.extend(
            entries
                .flatten()
                .filter_map(|entry| entry.path().into_os_string().into_string().ok()),
        );
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn run(
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    control: &Control,
) -> Result<(), mpsc::SendError<Event>> {
    let mut port = None;
    let mut buf = [0; 4096];
    while !control.stop.load(SeqCst) {
        let command = if port.is_some() {
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
                drop(port.take());
                match open(&settings) {
                    Ok(endpoint) => {
                        port = Some(endpoint);
                        events.send(Event::Opened(settings))?;
                    }
                    Err(error) => events.send(Event::OpenFailed(error.to_string()))?,
                }
            }
            Some(Command::Close) => {
                drop(port.take());
                events.send(Event::Closed)?;
            }
            Some(Command::Write(data, epoch)) if epoch == control.tx_epoch.load(SeqCst) => {
                if let Some(endpoint) = &mut port {
                    let (count, error) = write_packet(endpoint.as_mut(), &data, epoch, control);
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
                            drop(port.take());
                            events.send(Event::Lost(error.to_string()))?;
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(endpoint) = &mut port {
            match endpoint.read(&mut buf) {
                Ok(0) => {
                    drop(port.take());
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
                    drop(port.take());
                    events.send(Event::Lost(error.to_string()))?;
                }
            }
        }
    }
    Ok(())
}

fn write_packet(
    port: &mut dyn SerialPort,
    data: &[u8],
    epoch: usize,
    control: &Control,
) -> (usize, Option<io::Error>) {
    let mut count = 0;
    while count < data.len() && epoch == control.tx_epoch.load(SeqCst) {
        match port.write(&data[count..]) {
            Ok(0) => return (count, Some(io::ErrorKind::WriteZero.into())),
            Ok(size) => count += size,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return (count, Some(error)),
        }
    }
    (count, None)
}
