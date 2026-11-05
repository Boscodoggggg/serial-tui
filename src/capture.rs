use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Capture {
    path: PathBuf,
    sender: Option<SyncSender<String>>,
    writer: JoinHandle<io::Result<()>>,
    overflowed: bool,
}

impl Capture {
    pub fn start(path: PathBuf) -> io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let (sender, records) = mpsc::sync_channel(256);
        let writer = thread::Builder::new()
            .name("serial-capture".into())
            .spawn(move || write_records(BufWriter::with_capacity(64 * 1024, file), records))?;
        Ok(Self {
            path,
            sender: Some(sender),
            writer,
            overflowed: false,
        })
    }

    pub fn is_recording(&self) -> bool {
        self.sender.is_some()
    }

    pub fn is_finished(&self) -> bool {
        self.writer.is_finished()
    }

    pub fn push(&mut self, line: String) {
        let Some(sender) = &self.sender else { return };
        match sender.try_send(line) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.overflowed = true;
                self.stop();
            }
            Err(TrySendError::Disconnected(_)) => self.stop(),
        }
    }

    pub fn stop(&mut self) {
        drop(self.sender.take());
    }

    pub fn finish(mut self) -> (PathBuf, io::Result<bool>) {
        self.stop();
        let result = self
            .writer
            .join()
            .map_err(|_| io::Error::other("capture writer stopped unexpectedly"))
            .and_then(|result| result)
            .map(|()| self.overflowed);
        (self.path, result)
    }
}

fn write_records(mut writer: impl Write, records: Receiver<String>) -> io::Result<()> {
    const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
    let mut flush_at = Instant::now() + FLUSH_INTERVAL;
    loop {
        match records.recv_timeout(flush_at.saturating_duration_since(Instant::now())) {
            Ok(line) => writer.write_all(line.as_bytes())?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return writer.flush(),
        }
        if Instant::now() >= flush_at {
            writer.flush()?;
            flush_at = Instant::now() + FLUSH_INTERVAL;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_drains_queued_rows_and_an_existing_file_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("capture.log");
        let mut capture = Capture::start(path.clone()).unwrap();
        capture.push("time\tRX\t00 1B\n".into());
        capture.push("time\tTX\tFF\n".into());
        assert!(!capture.finish().1.unwrap());
        let error = Capture::start(path.clone()).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "time\tRX\t00 1B\ntime\tTX\tFF\n"
        );
    }
}
