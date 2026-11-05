mod app;
mod capture;
mod serial;
mod settings_store;
mod ui;

use std::{
    env,
    io::{self, IsTerminal},
    time::Duration,
};

use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind},
    execute,
};
use ratatui::DefaultTerminal;

use crate::app::{App, Language};

const USAGE: &str = "Usage: serial-tui [--port PATH] [--baud RATE] [--lang en|zh]

  --port PATH  Preselect a device path (connect inside the TUI)
  --baud RATE  Override the saved baud rate (initial default: 115200)
  --lang       Override the terminal locale
  -h, --help   Show this help
  -V, --version  Print the version";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut port = None;
    let mut baud = None;
    let mut lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.is_empty()))
        .filter(|locale| locale.to_ascii_lowercase().starts_with("zh"))
        .map_or(Language::En, |_| Language::Zh);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("serial-tui {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--port" => {
                port = Some(
                    args.next()
                        .filter(|port| !port.is_empty())
                        .ok_or("--port requires a device path")?,
                );
            }
            "--baud" => {
                baud = Some(
                    args.next()
                        .and_then(|value| value.parse().ok())
                        .filter(|rate| *rate > 0)
                        .ok_or("--baud requires a positive integer")?,
                );
            }
            "--lang" => {
                lang = match args.next().as_deref() {
                    Some("en") => Language::En,
                    Some("zh") => Language::Zh,
                    _ => return Err("--lang must be en or zh".into()),
                }
            }
            _ => return Err(format!("unknown option {arg}; use --help").into()),
        }
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("serial-tui requires an interactive terminal".into());
    }
    let mut app = App::new(lang, port, baud)?;
    let result = {
        let _restore = RestoreTerminal;
        let mut terminal = ratatui::try_init()?;
        execute!(io::stdout(), EnableBracketedPaste)?;
        run(&mut terminal, &mut app)
    };
    let capture_result = app.finish_capture_on_exit();
    result?;
    capture_result?;
    Ok(())
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    let mut dirty = true;
    loop {
        dirty |= app.tick();
        if dirty {
            terminal.draw(|frame| ui::draw(frame, app))?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(25))? {
            continue;
        }
        match event::read()? {
            Event::Key(key)
                if key.kind == KeyEventKind::Press
                    || (key.kind == KeyEventKind::Repeat
                        && !matches!(key.code, KeyCode::Enter | KeyCode::F(_))) =>
            {
                let size = terminal.size()?;
                if app.key(
                    key,
                    ui::log_height(
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        app.mode,
                    ),
                ) {
                    return Ok(());
                }
                dirty = true;
            }
            Event::Paste(value) => {
                app.paste(&value);
                dirty = true;
            }
            Event::Resize(_, _) => dirty = true,
            _ => {}
        }
    }
}
