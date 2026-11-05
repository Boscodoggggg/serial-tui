use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::serial::Settings;

pub fn path() -> Option<PathBuf> {
    absolute_env_path("XDG_STATE_HOME")
        .or_else(|| absolute_env_path("HOME").map(|home| home.join(".local/state")))
        .map(|dir| dir.join("serial-tui/last-connection.json"))
}

fn absolute_env_path(variable: &str) -> Option<PathBuf> {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub fn load(path: &Path) -> io::Result<Settings> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(error) => return Err(error),
    };
    let settings: Settings = serde_json::from_reader(io::BufReader::new(file))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if settings.port.is_empty() || settings.baud == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid serial settings",
        ));
    }
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut file, settings).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_replaces_the_last_successful_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serial-tui/last-connection.json");
        assert_eq!(load(&path).unwrap(), Settings::default());
        let mut settings = Settings {
            port: "/dev/serial/by-id/串口 adapter".into(),
            baud: 57_600,
            parity: serialport::Parity::Even,
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        settings.baud = 9_600;
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
    }
}
