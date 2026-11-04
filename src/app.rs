use std::{
    collections::VecDeque,
    fmt::Write as _,
    fs::OpenOptions,
    io::{BufWriter, Write},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::serial::{Command, Event, Settings, Worker, data_bits_number, stop_bits_number};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Zh,
}

impl Language {
    pub fn pick<'a>(self, en: &'a str, zh: &'a str) -> &'a str {
        match self {
            Self::En => en,
            Self::Zh => zh,
        }
    }

    pub fn text(self, key: &str) -> &str {
        let (en, zh) = match key {
            "title" => ("Serial Workbench", "串口调测台"),
            "settings" => ("Serial settings", "串口设置"),
            "port" => ("Port", "串口"),
            "baud" => ("Baud rate", "波特率"),
            "bits" => ("Data bits", "数据位"),
            "stop" => ("Stop bits", "停止位"),
            "parity" => ("Parity", "校验位"),
            "flow" => ("Flow control", "流控"),
            "none" => ("None", "无"),
            "even" => ("Even", "偶校验"),
            "odd" => ("Odd", "奇校验"),
            "manual_port" => ("Enter device path…", "输入设备路径…"),
            "custom_baud" => ("Custom baud rate…", "自定义波特率…"),
            "select_port" => ("Select a port", "请选择串口"),
            "connect" => ("Connect", "连接"),
            "apply" => ("Apply / Reconnect", "应用 / 重连"),
            "scan" => ("Refresh ports", "刷新串口"),
            "disconnect" => ("Disconnect", "断开"),
            "online" => ("● Connected", "● 已连接"),
            "offline" => ("○ Disconnected", "○ 已断开"),
            "pending" => ("Switching connection…", "正在切换连接…"),
            "changed" => ("Apply settings to reconnect", "参数已修改，请应用并重连"),
            "mode" => ("Mode", "工作模式"),
            "both" => ("Duplex", "收发"),
            "rx" => ("Receive only", "仅接收"),
            "tx" => ("Send only", "仅发送"),
            "both_log" => ("Session log", "收发日志"),
            "rx_log" => ("Receive log", "接收日志"),
            "tx_log" => ("Send log", "发送日志"),
            "following" => ("Following", "跟随最新"),
            "paused" => ("Paused", "已暂停"),
            "empty_log" => ("No records yet", "暂无记录"),
            "text" => ("Text", "文本"),
            "send" => ("Send", "发送区"),
            "send_now" => ("Send", "发送"),
            "format" => ("Format", "格式"),
            "eol" => ("EOL", "行尾"),
            "presets" => ("Presets", "常用指令"),
            "probe" => ("Binary probe", "二进制探测帧"),
            "repeat" => ("Repeat", "循环发送"),
            "off" => ("Off", "关闭"),
            "type" => ("Type a command", "输入指令"),
            "opened" => ("Opened", "已打开"),
            "closed" => ("Port closed", "已断开串口"),
            "lost" => ("Port lost", "串口连接丢失"),
            "open_failed" => ("Could not open port", "无法打开串口"),
            "scan_failed" => ("Could not list ports", "无法枚举串口"),
            "found" => ("Ports found", "发现串口数"),
            "write_failed" => ("Write timed out", "串口写入超时"),
            "busy" => ("Serial worker busy, try again", "串口繁忙，请重试"),
            "worker_down" => ("Serial worker stopped", "串口线程已停止"),
            "empty_send" => ("Enter something to send", "请输入要发送的内容"),
            "bad_hex" => (
                "Enter whole HEX bytes, e.g. 41 54 0D 0A",
                "请输入完整 HEX 字节，例如 41 54 0D 0A",
            ),
            "bad_baud" => ("Enter a positive baud rate", "请输入大于零的波特率"),
            "export" => ("Export log", "导出日志"),
            "export_path" => ("Export log to a new file", "将日志导出到新文件"),
            "saved" => ("Log exported", "已导出日志"),
            "save_failed" => ("Export failed", "日志导出失败"),
            "switch" => ("Switch pane", "切区"),
            "select" => ("Select", "选择"),
            "edit" => ("Edit", "编辑"),
            "confirm" => ("Confirm", "确认"),
            "cancel" => ("Cancel", "取消"),
            "page" => ("Page", "翻页"),
            "latest" => ("Latest", "最新"),
            "pan" => ("Pan", "横移"),
            "view" => ("View", "显示"),
            "history" => ("History", "发送历史"),
            "quit" => ("Quit", "退出"),
            "min_size" => (
                "Resize to at least 64 × 17. Ctrl+Q quits.",
                "请将终端调到至少 64 × 17。Ctrl+Q 退出。",
            ),
            _ => return key,
        };
        self.pick(en, zh)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Both,
    Rx,
    Tx,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Both, Self::Rx, Self::Tx];

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Rx => "rx",
            Self::Tx => "tx",
        }
    }

    pub fn log_title(self) -> &'static str {
        match self {
            Self::Both => "both_log",
            Self::Rx => "rx_log",
            Self::Tx => "tx_log",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Settings,
    Log,
    Options,
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Hex,
    Both,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Hex => "HEX",
            Self::Both => "HEX + ASCII",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    None,
    Lf,
    Cr,
    CrLf,
}

impl Ending {
    const ALL: [Self; 4] = [Self::None, Self::Lf, Self::Cr, Self::CrLf];

    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lf => "LF",
            Self::Cr => "CR",
            Self::CrLf => "CRLF",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::None => b"",
            Self::Lf => b"\n",
            Self::Cr => b"\r",
            Self::CrLf => b"\r\n",
        }
    }
}

#[derive(Clone, Default)]
pub struct Editor {
    pub value: String,
    pub cursor: usize,
}

impl Editor {
    fn new(value: String) -> Self {
        Self {
            cursor: value.len(),
            value,
        }
    }

    fn insert(&mut self, input: &str) {
        let remaining = 1024_usize.saturating_sub(self.value.len());
        let mut clean = String::new();
        for character in input.chars().filter(|character| !character.is_control()) {
            if clean.len() + character.len_utf8() > remaining {
                break;
            }
            clean.push(character);
        }
        self.value.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    fn key(&mut self, event: KeyEvent) {
        let previous = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(idx, _)| idx);
        let next = self.cursor
            + self.value[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        match event.code {
            KeyCode::Left => self.cursor = previous,
            KeyCode::Right => self.cursor = next,
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.len(),
            KeyCode::Backspace => {
                self.value.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete => {
                self.value.drain(self.cursor..next);
            }
            KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                *self = Self::default()
            }
            KeyCode::Char(c)
                if !event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(c.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct DraftMessage {
    input: String,
    format: Format,
    ending: Ending,
}

impl DraftMessage {
    fn text(input: &str) -> Self {
        Self {
            input: input.into(),
            format: Format::Text,
            ending: Ending::CrLf,
        }
    }
}

fn encode(message: &DraftMessage) -> Result<Vec<u8>, &'static str> {
    if message.input.is_empty() {
        return Err("empty_send");
    }
    if message.format == Format::Text {
        return Ok([message.input.as_bytes(), message.ending.bytes()].concat());
    }
    let hex: Vec<_> = message
        .input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    let (pairs, remainder) = hex.as_chunks::<2>();
    if pairs.is_empty() || !remainder.is_empty() {
        return Err("bad_hex");
    }
    pairs
        .iter()
        .map(|&[high, low]| {
            let high = (high as char).to_digit(16).ok_or("bad_hex")?;
            let low = (low as char).to_digit(16).ok_or("bad_hex")?;
            Ok((high * 16 + low) as u8)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    System,
    Rx,
    Tx,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "SYS",
            Self::Rx => "RX",
            Self::Tx => "TX",
        }
    }
}

pub struct LogEntry {
    pub id: u64,
    pub time: DateTime<Local>,
    pub direction: Direction,
    data: Vec<u8>,
    message: Option<(&'static str, String)>,
}

impl LogEntry {
    pub fn body(&self, lang: Language, display: Format) -> String {
        if let Some((key, detail)) = &self.message {
            let title = lang.text(key);
            return clean_text(if detail.is_empty() {
                title.into()
            } else {
                format!("{title} · {detail}")
            });
        }
        match display {
            Format::Text => {
                let text = String::from_utf8_lossy(&self.data);
                clean_text(text.trim_end_matches(['\r', '\n']).into())
            }
            Format::Hex => hex(&self.data),
            Format::Both => {
                let ascii: String = self
                    .data
                    .iter()
                    .map(|&byte| {
                        if (32..=126).contains(&byte) {
                            byte as char
                        } else {
                            '.'
                        }
                    })
                    .collect();
                format!("{}  │{ascii}│", hex(&self.data))
            }
        }
    }
}

pub fn clean_text(value: String) -> String {
    let mut output = String::new();
    for c in value.chars() {
        if c.is_control() {
            let _ = write!(output, "\\x{:02X}", c as u32);
        } else {
            output.push(c);
        }
    }
    output
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output.push(' ');
        }
        let _ = write!(output, "{byte:02X}");
    }
    output
}

pub struct Log {
    entries: VecDeque<LogEntry>,
    sequence: u64,
    pub anchor: Option<u64>,
    pub horizontal: usize,
    pub rx: u64,
    pub tx: u64,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            sequence: 1,
            anchor: None,
            horizontal: 0,
            rx: 0,
            tx: 0,
        }
    }
}

impl Log {
    fn push(
        &mut self,
        direction: Direction,
        data: Vec<u8>,
        message: Option<(&'static str, String)>,
    ) {
        self.entries.push_back(LogEntry {
            id: self.sequence,
            time: Local::now(),
            direction,
            data,
            message,
        });
        self.sequence += 1;
        if self.entries.len() > 2000 {
            self.entries.pop_front();
        }
    }

    fn system(&mut self, key: &'static str, detail: String) {
        self.push(Direction::System, Vec::new(), Some((key, detail)));
    }

    fn receive(&mut self, bytes: &[u8]) {
        self.rx += bytes.len() as u64;
        for mut line in bytes.split_inclusive(|byte| *byte == b'\n') {
            if let Some(previous) = self.entries.back_mut().filter(|entry| {
                entry.direction == Direction::Rx
                    && !entry.data.ends_with(b"\n")
                    && self.anchor.is_none_or(|anchor| entry.id > anchor)
            }) {
                let take = line.len().min(512 - previous.data.len());
                previous.data.extend_from_slice(&line[..take]);
                line = &line[take..];
            }
            for chunk in line.chunks(512) {
                self.push(Direction::Rx, chunk.to_vec(), None);
            }
        }
    }

    fn sent(&mut self, bytes: Vec<u8>) {
        self.tx += bytes.len() as u64;
        self.push(Direction::Tx, bytes, None);
    }

    fn filtered(&self, mode: Mode) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|entry| match entry.direction {
                Direction::System => true,
                Direction::Rx => mode != Mode::Tx,
                Direction::Tx => mode != Mode::Rx,
            })
            .collect()
    }

    fn end(&self, entries: &[&LogEntry]) -> usize {
        self.anchor.map_or(entries.len(), |id| {
            entries.partition_point(|entry| entry.id <= id)
        })
    }

    pub fn visible(&self, mode: Mode, height: usize) -> Vec<&LogEntry> {
        let entries = self.filtered(mode);
        let end = self.end(&entries);
        entries[end.saturating_sub(height)..end].to_vec()
    }

    fn scroll(&mut self, mode: Mode, height: usize, delta: isize) {
        let entries = self.filtered(mode);
        let end = self
            .end(&entries)
            .saturating_add_signed(delta)
            .clamp(height.min(entries.len()), entries.len());
        self.anchor = if end == entries.len() && delta > 0 {
            None
        } else {
            Some(end.checked_sub(1).map_or(0, |index| entries[index].id))
        };
    }

    fn toggle_follow(&mut self) {
        self.anchor = if self.anchor.is_some() {
            None
        } else {
            Some(self.sequence - 1)
        };
    }

    fn export(&self, path: &str, lang: Language) -> std::io::Result<()> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = BufWriter::new(file);
        for entry in &self.entries {
            writeln!(
                writer,
                "{}\t{}\t{}",
                entry.time.to_rfc3339(),
                entry.direction.label(),
                entry.body(lang, Format::Hex)
            )?;
        }
        writer.flush()
    }
}

#[derive(Clone)]
enum Value {
    Port(String),
    Baud(u32),
    Data(DataBits),
    Stop(StopBits),
    Parity(Parity),
    Flow(FlowControl),
    SendFormat(Format),
    Ending(Ending),
    Preset(DraftMessage),
    Repeat(u64),
    Edit(EditField),
}

#[derive(Clone, Copy)]
pub enum EditField {
    Port,
    Baud,
    Export,
}

pub struct Menu {
    pub title: &'static str,
    choices: Vec<(String, Value)>,
    pub selected: usize,
}

impl Menu {
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.choices.iter().map(|(label, _)| label.as_str())
    }
}

pub enum Modal {
    Menu(Menu),
    Edit(EditField, Editor),
}

impl EditField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Port => "manual_port",
            Self::Baud => "custom_baud",
            Self::Export => "export_path",
        }
    }
}

struct Repeat {
    bytes: Vec<u8>,
    interval: Duration,
    next: Instant,
}

pub struct App {
    worker: Worker,
    pub lang: Language,
    pub draft: Settings,
    pub active: Option<Settings>,
    pub pending: bool,
    pub mode: Mode,
    pub pane: Pane,
    pub setting: usize,
    pub option: usize,
    pub modal: Option<Modal>,
    pub input: Editor,
    pub log: Log,
    pub display: Format,
    ports: Vec<String>,
    notice: Option<(&'static str, String)>,
    format: Format,
    ending: Ending,
    history: VecDeque<DraftMessage>,
    history_index: Option<usize>,
    before_history: Option<DraftMessage>,
    repeat: Option<Repeat>,
}

impl App {
    pub fn new(lang: Language, draft: Settings) -> std::io::Result<Self> {
        let mut app = Self {
            worker: Worker::start()?,
            lang,
            draft,
            active: None,
            pending: false,
            mode: Mode::Both,
            pane: Pane::Settings,
            setting: 0,
            option: 0,
            modal: None,
            input: Editor::default(),
            log: Log::default(),
            display: Format::Text,
            ports: Vec::new(),
            notice: None,
            format: Format::Text,
            ending: Ending::CrLf,
            history: VecDeque::new(),
            history_index: None,
            before_history: None,
            repeat: None,
        };
        app.enqueue(Command::Scan);
        Ok(app)
    }

    fn notice(&mut self, key: &'static str, detail: String) {
        self.notice = Some((key, detail));
    }

    pub fn message(&self) -> String {
        if let Some((key, detail)) = &self.notice {
            let title = self.lang.text(key);
            return clean_text(if detail.is_empty() {
                title.into()
            } else {
                format!("{title} · {detail}")
            });
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active != &self.draft)
        {
            return self.lang.text("changed").into();
        }
        String::new()
    }

    fn enqueue(&mut self, command: Command) -> bool {
        if let Err(key) = self.worker.enqueue(command) {
            self.notice(key, String::new());
            return false;
        }
        true
    }

    fn connect(&mut self) {
        if self.pending {
            return;
        }
        if self.draft.port.is_empty() {
            self.setting = 0;
            self.activate_setting();
            return;
        }
        self.repeat = None;
        self.worker.invalidate_writes();
        self.pending = self.enqueue(Command::Open(self.draft.clone()));
    }

    fn disconnect(&mut self) {
        if self.pending || self.active.is_none() {
            return;
        }
        self.repeat = None;
        self.worker.invalidate_writes();
        self.pending = self.enqueue(Command::Close);
    }

    pub fn settings_rows(&self) -> [(String, String); 9] {
        let t = |key| self.lang.text(key).to_owned();
        [
            (
                t("port"),
                if self.draft.port.is_empty() {
                    t("select_port")
                } else {
                    clean_text(self.draft.port.clone())
                },
            ),
            (t("baud"), self.draft.baud.to_string()),
            (t("bits"), data_bits_number(self.draft.data).to_string()),
            (t("stop"), stop_bits_number(self.draft.stop).to_string()),
            (t("parity"), self.parity(self.draft.parity)),
            (t("flow"), self.flow(self.draft.flow)),
            (
                format!(
                    "[ {} ]",
                    t(if self.active.is_some() {
                        "apply"
                    } else {
                        "connect"
                    })
                ),
                String::new(),
            ),
            (format!("[ {} ]", t("scan")), String::new()),
            (format!("[ {} ]", t("disconnect")), String::new()),
        ]
    }

    pub fn options_rows(&self) -> [String; 4] {
        let t = |key| self.lang.text(key);
        [
            format!("{} {}", t("format"), t(self.format.label())),
            format!(
                "{} {}",
                t("eol"),
                if self.format == Format::Hex {
                    "—"
                } else {
                    t(self.ending.label())
                }
            ),
            t("presets").into(),
            format!(
                "{} {}",
                t("repeat"),
                self.repeat.as_ref().map_or_else(
                    || t("off").into(),
                    |repeat| format!("{} ms", repeat.interval.as_millis())
                )
            ),
        ]
    }

    fn parity(&self, parity: Parity) -> String {
        self.lang
            .text(match parity {
                Parity::None => "none",
                Parity::Even => "even",
                Parity::Odd => "odd",
            })
            .into()
    }

    fn flow(&self, flow: FlowControl) -> String {
        self.lang
            .text(match flow {
                FlowControl::None => "none",
                FlowControl::Hardware => "RTS/CTS",
                FlowControl::Software => "XON/XOFF",
            })
            .into()
    }

    fn menu(&mut self, title: &'static str, choices: Vec<(String, Value)>, selected: usize) {
        self.modal = Some(Modal::Menu(Menu {
            title,
            choices,
            selected,
        }));
    }

    fn activate_setting(&mut self) {
        const BAUD: [u32; 10] = [
            1200, 2400, 9600, 19_200, 38_400, 57_600, 115_200, 230_400, 460_800, 921_600,
        ];
        const DATA: [DataBits; 4] = [
            DataBits::Five,
            DataBits::Six,
            DataBits::Seven,
            DataBits::Eight,
        ];
        const STOP: [StopBits; 2] = [StopBits::One, StopBits::Two];
        const PARITY: [Parity; 3] = [Parity::None, Parity::Even, Parity::Odd];
        const FLOW: [FlowControl; 3] = [
            FlowControl::None,
            FlowControl::Hardware,
            FlowControl::Software,
        ];
        match self.setting {
            0 => {
                let mut ports = self.ports.clone();
                if !self.draft.port.is_empty() && !ports.contains(&self.draft.port) {
                    ports.insert(0, self.draft.port.clone());
                }
                let selected = index_of(&ports, &self.draft.port);
                let mut choices: Vec<_> = ports
                    .into_iter()
                    .map(|port| (clean_text(port.clone()), Value::Port(port)))
                    .collect();
                choices.push((
                    self.lang.text("manual_port").into(),
                    Value::Edit(EditField::Port),
                ));
                self.menu("port", choices, selected);
            }
            1 => {
                let mut choices: Vec<_> = BAUD
                    .into_iter()
                    .map(|baud| (baud.to_string(), Value::Baud(baud)))
                    .collect();
                let selected = BAUD
                    .iter()
                    .position(|baud| *baud == self.draft.baud)
                    .unwrap_or(choices.len());
                choices.push((
                    self.lang.text("custom_baud").into(),
                    Value::Edit(EditField::Baud),
                ));
                self.menu("baud", choices, selected);
            }
            2 => self.menu(
                "bits",
                DATA.into_iter()
                    .map(|bits| (data_bits_number(bits).to_string(), Value::Data(bits)))
                    .collect(),
                index_of(&DATA, &self.draft.data),
            ),
            3 => self.menu(
                "stop",
                STOP.into_iter()
                    .map(|stop| (stop_bits_number(stop).to_string(), Value::Stop(stop)))
                    .collect(),
                index_of(&STOP, &self.draft.stop),
            ),
            4 => self.menu(
                "parity",
                PARITY
                    .into_iter()
                    .map(|parity| (self.parity(parity), Value::Parity(parity)))
                    .collect(),
                index_of(&PARITY, &self.draft.parity),
            ),
            5 => self.menu(
                "flow",
                FLOW.into_iter()
                    .map(|flow| (self.flow(flow), Value::Flow(flow)))
                    .collect(),
                index_of(&FLOW, &self.draft.flow),
            ),
            6 => self.connect(),
            7 => {
                self.enqueue(Command::Scan);
            }
            8 => self.disconnect(),
            _ => {}
        }
    }

    fn activate_option(&mut self) {
        match self.option {
            0 => self.menu(
                "format",
                [Format::Text, Format::Hex]
                    .into_iter()
                    .map(|format| {
                        (
                            self.lang.text(format.label()).into(),
                            Value::SendFormat(format),
                        )
                    })
                    .collect(),
                usize::from(self.format == Format::Hex),
            ),
            1 if self.format == Format::Text => self.menu(
                "eol",
                Ending::ALL
                    .into_iter()
                    .map(|ending| (self.lang.text(ending.label()).into(), Value::Ending(ending)))
                    .collect(),
                index_of(&Ending::ALL, &self.ending),
            ),
            2 => self.menu(
                "presets",
                vec![
                    ("AT".into(), Value::Preset(DraftMessage::text("AT"))),
                    ("AT+GMR".into(), Value::Preset(DraftMessage::text("AT+GMR"))),
                    (
                        self.lang.text("probe").into(),
                        Value::Preset(DraftMessage {
                            input: "01 03 00 00 00 01 84 0A".into(),
                            format: Format::Hex,
                            ending: Ending::None,
                        }),
                    ),
                ],
                0,
            ),
            3 if self.repeat.is_some() => self.repeat = None,
            3 => self.menu(
                "repeat",
                [500, 1000, 2000]
                    .into_iter()
                    .map(|ms| (format!("{ms} ms"), Value::Repeat(ms)))
                    .collect(),
                1,
            ),
            _ => {}
        }
    }

    fn apply(&mut self, value: Value) {
        match value {
            Value::Port(port) => self.draft.port = port,
            Value::Baud(baud) => self.draft.baud = baud,
            Value::Data(bits) => self.draft.data = bits,
            Value::Stop(bits) => self.draft.stop = bits,
            Value::Parity(parity) => self.draft.parity = parity,
            Value::Flow(flow) => self.draft.flow = flow,
            Value::SendFormat(format) => self.format = format,
            Value::Ending(ending) => self.ending = ending,
            Value::Preset(preset) => {
                self.load(preset);
                self.history_index = None;
                self.pane = Pane::Input;
            }
            Value::Repeat(ms) => self.send(Some(Duration::from_millis(ms))),
            Value::Edit(field) => {
                let initial = match field {
                    EditField::Port => self.draft.port.clone(),
                    EditField::Baud => self.draft.baud.to_string(),
                    EditField::Export => {
                        Local::now().format("serial-%Y%m%d-%H%M%S.log").to_string()
                    }
                };
                self.modal = Some(Modal::Edit(field, Editor::new(initial)));
            }
        }
    }

    fn confirm_edit(&mut self, field: EditField, editor: Editor) {
        let value = editor.value.trim();
        let problem = match field {
            EditField::Port if value.is_empty() => Some(("select_port", String::new())),
            EditField::Port => {
                self.draft.port = value.into();
                None
            }
            EditField::Baud => match value.parse::<u32>() {
                Ok(baud) if baud > 0 => {
                    self.draft.baud = baud;
                    None
                }
                _ => Some(("bad_baud", String::new())),
            },
            EditField::Export => match self.log.export(value, self.lang) {
                Ok(()) => {
                    self.notice("saved", value.into());
                    None
                }
                Err(error) => Some(("save_failed", error.to_string())),
            },
        };
        if let Some((key, detail)) = problem {
            self.notice(key, detail);
            self.modal = Some(Modal::Edit(field, editor));
        }
    }

    fn modal_key(&mut self, event: KeyEvent) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        match &mut modal {
            _ if event.code == KeyCode::Esc => return,
            Modal::Menu(menu) => match event.code {
                KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
                KeyCode::Down => menu.selected = (menu.selected + 1).min(menu.choices.len() - 1),
                KeyCode::Home => menu.selected = 0,
                KeyCode::End => menu.selected = menu.choices.len() - 1,
                KeyCode::Enter => {
                    self.apply(menu.choices[menu.selected].1.clone());
                    return;
                }
                _ => {}
            },
            Modal::Edit(field, editor) => {
                if event.code == KeyCode::Enter {
                    self.confirm_edit(*field, std::mem::take(editor));
                    return;
                }
                editor.key(event);
            }
        }
        self.modal = Some(modal);
    }

    fn draft_message(&self) -> DraftMessage {
        DraftMessage {
            input: self.input.value.clone(),
            format: self.format,
            ending: self.ending,
        }
    }

    fn load(&mut self, message: DraftMessage) {
        self.input = Editor::new(message.input);
        self.format = message.format;
        self.ending = message.ending;
    }

    fn history(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        if older {
            if self.history_index.is_none() {
                self.before_history = Some(self.draft_message());
            }
            let index = self
                .history_index
                .map_or(0, |index| (index + 1).min(self.history.len() - 1));
            self.history_index = Some(index);
            self.load(self.history[index].clone());
        } else if let Some(index) = self.history_index {
            self.history_index = index.checked_sub(1);
            if let Some(index) = self.history_index {
                self.load(self.history[index].clone());
            } else if let Some(message) = self.before_history.take() {
                self.load(message);
            }
        }
    }

    fn send(&mut self, interval: Option<Duration>) {
        if self.mode == Mode::Rx || self.pending {
            return;
        }
        if self.active.is_none() {
            self.notice("select_port", String::new());
            return;
        }
        let message = self.draft_message();
        let bytes = match encode(&message) {
            Ok(bytes) => bytes,
            Err(key) => {
                self.notice(key, String::new());
                return;
            }
        };
        if let Err(key) = self.worker.write(bytes.clone()) {
            self.notice(key, String::new());
            return;
        }
        if self.history.front() != Some(&message) {
            self.history.push_front(message);
            self.history.truncate(20);
        }
        self.history_index = None;
        self.before_history = None;
        if let Some(interval) = interval {
            self.repeat = Some(Repeat {
                bytes,
                interval,
                next: Instant::now() + interval,
            });
        }
    }

    fn switch_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Both => Mode::Rx,
            Mode::Rx => Mode::Tx,
            Mode::Tx => Mode::Both,
        };
        self.worker.discard_rx(self.mode == Mode::Tx);
        if self.mode == Mode::Rx {
            self.repeat = None;
            self.worker.invalidate_writes();
            if matches!(self.pane, Pane::Options | Pane::Input) {
                self.pane = Pane::Log;
            }
        }
    }

    fn switch_pane(&mut self, reverse: bool) {
        let panes: &[Pane] = if self.mode == Mode::Rx {
            &[Pane::Settings, Pane::Log]
        } else {
            &[Pane::Settings, Pane::Log, Pane::Options, Pane::Input]
        };
        let index = index_of(panes, &self.pane);
        self.pane = panes[(index + if reverse { panes.len() - 1 } else { 1 }) % panes.len()];
    }

    pub fn paste(&mut self, input: &str) {
        if let Some(Modal::Edit(_, editor)) = &mut self.modal {
            editor.insert(input);
        } else if self.modal.is_none() && self.pane == Pane::Input {
            self.input.insert(input);
            self.history_index = None;
        }
    }

    // Returns true when the application should exit.
    pub fn key(&mut self, event: KeyEvent, log_height: usize) -> bool {
        if matches!(event.code, KeyCode::Char('q' | 'c'))
            && event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return true;
        }
        self.notice = None;
        if self.modal.is_some() {
            self.modal_key(event);
            return false;
        }
        match event.code {
            KeyCode::F(2) => self.pane = Pane::Settings,
            KeyCode::F(3) => self.switch_mode(),
            KeyCode::F(4) => self.log.toggle_follow(),
            KeyCode::F(6) => {
                self.lang = if self.lang == Language::En {
                    Language::Zh
                } else {
                    Language::En
                }
            }
            KeyCode::F(8) => self.apply(Value::Edit(EditField::Export)),
            KeyCode::Tab => self.switch_pane(event.modifiers.contains(KeyModifiers::SHIFT)),
            KeyCode::BackTab => self.switch_pane(true),
            key => match self.pane {
                Pane::Settings => match key {
                    KeyCode::Up => self.setting = self.setting.saturating_sub(1),
                    KeyCode::Down => self.setting = (self.setting + 1).min(8),
                    KeyCode::Enter => self.activate_setting(),
                    _ => {}
                },
                Pane::Log => match key {
                    KeyCode::Up => self.log.scroll(self.mode, log_height, -1),
                    KeyCode::Down => self.log.scroll(self.mode, log_height, 1),
                    KeyCode::PageUp => {
                        self.log
                            .scroll(self.mode, log_height, -(log_height as isize))
                    }
                    KeyCode::PageDown => {
                        self.log.scroll(self.mode, log_height, log_height as isize)
                    }
                    KeyCode::Home => self.log.scroll(self.mode, log_height, isize::MIN),
                    KeyCode::End => self.log.anchor = None,
                    KeyCode::Left => self.log.horizontal = self.log.horizontal.saturating_sub(8),
                    KeyCode::Right => self.log.horizontal = (self.log.horizontal + 8).min(5000),
                    KeyCode::Char('h' | 'H') => {
                        self.display = match self.display {
                            Format::Text => Format::Hex,
                            Format::Hex => Format::Both,
                            Format::Both => Format::Text,
                        };
                        self.log.horizontal = 0;
                    }
                    _ => {}
                },
                Pane::Options => match key {
                    KeyCode::Left => self.option = self.option.saturating_sub(1),
                    KeyCode::Right => self.option = (self.option + 1).min(3),
                    KeyCode::Enter => self.activate_option(),
                    _ => {}
                },
                Pane::Input => match key {
                    KeyCode::Enter => self.send(None),
                    KeyCode::Up => self.history(true),
                    KeyCode::Down => self.history(false),
                    _ => {
                        let old = self.input.value.clone();
                        self.input.key(event);
                        if self.input.value != old {
                            self.history_index = None;
                        }
                    }
                },
            },
        }
        false
    }

    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..128 {
            let Some(events) = &self.worker.events else {
                break;
            };
            let event = match events.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker.events = None;
                    self.active = None;
                    self.pending = false;
                    self.repeat = None;
                    self.notice("worker_down", String::new());
                    changed = true;
                    break;
                }
            };
            self.serial_event(event);
            changed = true;
        }
        let now = Instant::now();
        if let Some(repeat) = self.repeat.as_mut().filter(|repeat| now >= repeat.next) {
            repeat.next = now + repeat.interval;
            if let Err(key) = self.worker.write(repeat.bytes.clone()) {
                self.repeat = None;
                self.notice(key, String::new());
                changed = true;
            }
        }
        changed
    }

    fn serial_event(&mut self, event: Event) {
        match event {
            Event::Ports(ports) => {
                self.notice("found", ports.len().to_string());
                self.ports = ports;
            }
            Event::Opened(settings) => {
                self.log.system("opened", settings.summary());
                self.active = Some(settings);
                self.pending = false;
                self.notice = None;
            }
            Event::Closed => {
                self.log.system("closed", String::new());
                self.active = None;
                self.pending = false;
            }
            Event::OpenFailed(error) => {
                self.active = None;
                self.pending = false;
                self.log.system("open_failed", error.clone());
                self.notice("open_failed", error);
            }
            Event::Lost(error) => {
                self.active = None;
                self.repeat = None;
                self.log.system("lost", error.clone());
                self.notice("lost", error);
            }
            Event::ScanFailed(error) => self.notice("scan_failed", error),
            Event::WriteFailed(error) => {
                self.repeat = None;
                self.log.system("write_failed", error.clone());
                self.notice("write_failed", error);
            }
            Event::Rx(bytes) => self.log.receive(&bytes),
            Event::Tx(bytes) => self.log.sent(bytes),
        }
    }
}

fn index_of<T: PartialEq>(values: &[T], selected: &T) -> usize {
    values
        .iter()
        .position(|value| value == selected)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_send_preserves_binary_bytes_without_adding_eol() {
        let mut message = DraftMessage::text("00 ff 1B");
        message.format = Format::Hex;
        assert_eq!(encode(&message), Ok(vec![0, 255, 27]));
        message.input = "00 f".into();
        assert_eq!(encode(&message), Err("bad_hex"));
    }

    #[test]
    fn receive_reassembles_utf8_and_does_not_send_control_bytes_to_the_terminal() {
        let mut log = Log::default();
        let data = "串\x1b[2J\r\n".as_bytes();
        log.receive(&data[..2]);
        log.receive(&data[2..]);
        let row = &log.visible(Mode::Both, 10)[0];
        assert_eq!(row.body(Language::En, Format::Text), "串\\x1B[2J");
        assert_eq!(row.data, data);
    }

    #[test]
    fn paused_log_keeps_capturing_bytes_without_mutating_its_visible_rows() {
        let mut log = Log::default();
        log.receive(b"before");
        log.toggle_follow();
        log.receive(b"after\n");
        assert_eq!(log.rx, 12);
        assert_eq!(log.visible(Mode::Rx, 10)[0].data, b"before");
        assert_eq!(log.visible(Mode::Rx, 10).len(), 1);
        log.toggle_follow();
        assert_eq!(log.visible(Mode::Rx, 10)[1].data, b"after\n");
    }
}
