use std::{
    collections::VecDeque,
    fmt::Write as _,
    fs::OpenOptions,
    io::{BufWriter, Write},
    ops::Range,
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::{
    capture::Capture,
    serial::{
        Command, Event, PortFailure, PortFailureKind, PortInfo, Settings, Worker, data_bits_number,
        stop_bits_number,
    },
    settings_store,
};

mod search;
use search::{LogSearch, SearchInput};

const CONNECT_ROW: usize = 6;

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
            "reconnect" => ("Reconnect", "应用 / 重连"),
            "scan" => ("Refresh ports", "刷新串口"),
            "refresh" => ("Refresh", "刷新"),
            "scanning" => ("Refreshing…", "正在刷新…"),
            "usb_port" => ("USB serial", "USB 转串口"),
            "uart_port" => ("Onboard UART", "板载 UART"),
            "pci_port" => ("PCI serial", "PCI 串口"),
            "bluetooth_port" => ("Bluetooth", "蓝牙"),
            "current_port" => ("Connected", "当前连接"),
            "configured_port" => ("Configured path", "已设路径"),
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
            "port_permission" => ("Port access denied", "串口权限不足"),
            "port_busy" => ("Port in use", "串口被占用"),
            "port_missing" => ("Port missing", "串口设备不存在"),
            "port_settings" => ("Serial settings rejected", "串口参数设置失败"),
            "port_disconnected" => ("Device disconnected", "串口设备已断开"),
            "connection_kept" => ("Previous connection available", "原连接仍可用"),
            "still_connected" => ("Still connected", "仍连接"),
            "restore_failed" => ("Could not restore previous connection", "无法恢复原连接"),
            "settings_load_failed" => ("Could not load saved settings", "无法读取上次的串口参数"),
            "settings_save_failed" => ("Could not save settings", "无法保存串口参数"),
            "scan_failed" => ("Could not list ports", "无法枚举串口"),
            "found" => ("Ports found", "发现串口数"),
            "write_failed" => ("Write timed out", "串口写入超时"),
            "busy" => ("Serial worker busy, try again", "串口繁忙，请重试"),
            "worker_down" => ("Serial worker stopped", "串口线程已停止"),
            "empty_send" => ("Enter something to send", "请输入要发送的内容"),
            "connect_first" => ("Connect to a port first", "请先连接串口"),
            "bad_hex" => (
                "Enter whole HEX bytes, e.g. 41 54 0D 0A",
                "请输入完整 HEX 字节，例如 41 54 0D 0A",
            ),
            "bad_baud" => ("Enter a positive baud rate", "请输入大于零的波特率"),
            "export" => ("Export log", "导出日志"),
            "export_path" => ("Export log to a new file", "将日志导出到新文件"),
            "saved" => ("Log exported", "已导出日志"),
            "save_failed" => ("Export failed", "日志导出失败"),
            "capture_path" => ("Record traffic to a new file", "连续记录到新文件"),
            "capture_started" => ("Recording started", "已开始连续录制"),
            "capture_saved" => ("Recording saved", "已保存录制日志"),
            "capture_failed" => ("Recording failed", "日志录制失败"),
            "capture_overflow" => (
                "Recording stopped: disk writer fell behind",
                "写入跟不上，已停止录制",
            ),
            "switch" => ("Switch pane", "切区"),
            "select" => ("Select", "选择"),
            "edit" => ("Edit", "编辑"),
            "confirm" => ("Confirm", "确认"),
            "cancel" => ("Cancel", "取消"),
            "page" => ("Page", "翻页"),
            "latest" => ("Latest", "最新"),
            "pan" => ("Pan", "横移"),
            "view" => ("View", "显示"),
            "search" => ("Find", "搜索"),
            "search_log" => ("Find in log", "搜索日志"),
            "search_empty" => ("Enter a search term", "请输入搜索内容"),
            "search_not_found" => ("No matches in the log", "日志中未找到匹配项"),
            "search_all" => ("All directions", "全部方向"),
            "search_rx" => ("RX only", "仅接收"),
            "search_scope" => ("Scope", "范围"),
            "search_jump" => ("Jump", "跳转"),
            "search_clear" => ("Clear find", "清除搜索"),
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
    decode_hex(&message.input)
}

fn decode_hex(input: &str) -> Result<Vec<u8>, &'static str> {
    let hex: Vec<_> = input
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

#[derive(Clone)]
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
            return system_text(lang, key, detail);
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

fn system_text(lang: Language, key: &str, detail: &str) -> String {
    let title = lang.text(key);
    clean_text(if detail.is_empty() {
        title.into()
    } else {
        format!("{title} · {detail}")
    })
}

fn log_line(time: &DateTime<Local>, direction: Direction, body: &str) -> String {
    format!("{}\t{}\t{body}\n", time.to_rfc3339(), direction.label())
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

struct PausedLog {
    entries: VecDeque<LogEntry>,
    anchor: u64,
}

pub struct Log {
    entries: VecDeque<LogEntry>,
    sequence: u64,
    paused: Option<PausedLog>,
    pub horizontal: usize,
    pub rx: u64,
    pub tx: u64,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            sequence: 1,
            paused: None,
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
            if let Some(previous) = self
                .entries
                .back_mut()
                .filter(|entry| entry.direction == Direction::Rx && !entry.data.ends_with(b"\n"))
            {
                let take = rx_prefix_len(line, previous.data.len());
                previous.data.extend_from_slice(&line[..take]);
                line = &line[take..];
            }
            while !line.is_empty() {
                let (chunk, rest) = line.split_at(rx_prefix_len(line, 0));
                self.push(Direction::Rx, chunk.to_vec(), None);
                line = rest;
            }
        }
    }

    fn sent(&mut self, bytes: Vec<u8>) {
        self.tx += bytes.len() as u64;
        self.push(Direction::Tx, bytes, None);
    }

    fn filtered(&self, mode: Mode) -> Vec<&LogEntry> {
        self.paused
            .as_ref()
            .map_or(&self.entries, |paused| &paused.entries)
            .iter()
            .filter(|entry| match entry.direction {
                Direction::System => true,
                Direction::Rx => mode != Mode::Tx,
                Direction::Tx => mode != Mode::Rx,
            })
            .collect()
    }

    fn end(&self, entries: &[&LogEntry]) -> usize {
        self.paused.as_ref().map_or(entries.len(), |paused| {
            entries.partition_point(|entry| entry.id <= paused.anchor)
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
        if end == entries.len() && delta > 0 {
            self.follow();
        } else {
            let anchor = end.checked_sub(1).map_or(0, |index| entries[index].id);
            self.pause_at(anchor);
        }
    }

    fn toggle_follow(&mut self) {
        if self.is_paused() {
            self.follow();
        } else {
            self.pause_at(self.sequence - 1);
        }
    }

    fn pause_at(&mut self, anchor: u64) {
        if let Some(paused) = &mut self.paused {
            paused.anchor = anchor;
        } else {
            self.paused = Some(PausedLog {
                entries: self.entries.clone(),
                anchor,
            });
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    fn follow(&mut self) {
        self.paused = None;
    }

    fn export(&self, path: &str, lang: Language) -> std::io::Result<()> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = BufWriter::new(file);
        for entry in &self.entries {
            writer.write_all(
                log_line(&entry.time, entry.direction, &entry.body(lang, Format::Hex)).as_bytes(),
            )?;
        }
        writer.flush()
    }
}

fn rx_prefix_len(bytes: &[u8], used: usize) -> usize {
    const RECORD_BYTES: usize = 512;
    let mut end = bytes.len().min(RECORD_BYTES.saturating_sub(used));
    // Allow a UTF-8 scalar that straddles the boundary to finish in this record.
    let max = bytes.len().min((RECORD_BYTES + 3).saturating_sub(used));
    while end < max && bytes[end] & 0b1100_0000 == 0b1000_0000 {
        end += 1;
    }
    end
}

#[derive(Clone)]
enum Value {
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
    Capture,
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

pub struct PortMenu {
    pub ports: Vec<PortInfo>,
    pub configured: Option<String>,
    pub selected: usize,
    offset: usize,
}

impl PortMenu {
    fn new(ports: Vec<PortInfo>, draft: &str) -> Self {
        let matched = ports.iter().position(|port| port.matches(draft));
        let configured = (matched.is_none() && !draft.is_empty()).then(|| draft.to_owned());
        let selected =
            matched.unwrap_or_else(|| if configured.is_some() { ports.len() } else { 0 });
        Self {
            ports,
            configured,
            selected,
            offset: 0,
        }
    }

    pub fn manual_index(&self) -> usize {
        self.ports.len() + usize::from(self.configured.is_some())
    }

    pub fn selected_path(&self) -> Option<&str> {
        self.ports
            .get(self.selected)
            .map(|port| port.path.as_str())
            .or(self
                .configured
                .as_deref()
                .filter(|_| self.selected == self.ports.len()))
    }

    pub fn visible(&self, page: usize) -> Range<usize> {
        let len = self.manual_index() + 1;
        let offset = self
            .offset
            .min(len.saturating_sub(page))
            .min(self.selected)
            .max((self.selected + 1).saturating_sub(page));
        offset..(offset + page).min(len)
    }

    fn refresh(&mut self, ports: Vec<PortInfo>, draft: &str) {
        let mut next = Self::new(ports, draft);
        let previous_device = self.ports.get(self.selected);
        next.selected = match self.selected_path() {
            None if self.manual_index() > 0 => next.manual_index(),
            None => next.selected,
            Some(path) => next
                .ports
                .iter()
                .position(|port| port.path == path)
                .or_else(|| {
                    next.ports.iter().position(|port| match previous_device {
                        Some(previous) => port.matches_previous(previous),
                        None => port.matches(path),
                    })
                })
                .or_else(|| {
                    (self.selected == self.ports.len() && next.configured.as_deref() == Some(path))
                        .then_some(next.ports.len())
                })
                .unwrap_or_else(|| {
                    next.ports
                        .len()
                        .checked_sub(1)
                        .map_or(next.manual_index(), |last| self.selected.min(last))
                }),
        };
        next.offset = self.offset;
        *self = next;
    }
}

pub enum Modal {
    Menu(Menu),
    Ports(PortMenu),
    Edit(EditField, Editor),
    Search(SearchInput),
}

impl EditField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Port => "manual_port",
            Self::Baud => "custom_baud",
            Self::Export => "export_path",
            Self::Capture => "capture_path",
        }
    }
}

struct Repeat {
    bytes: Vec<u8>,
    interval: Duration,
    next: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstOpen {
    NotYet,
    FocusOnSuccess,
    Complete,
}

pub struct App {
    worker: Worker,
    settings_path: Option<PathBuf>,
    capture: Option<Capture>,
    pub lang: Language,
    pub draft: Settings,
    pub active: Option<Settings>,
    pub pending: bool,
    first_open: FirstOpen,
    pub mode: Mode,
    pub pane: Pane,
    pub setting: usize,
    pub option: usize,
    pub modal: Option<Modal>,
    pub input: Editor,
    pub log: Log,
    pub display: Format,
    search: Option<LogSearch>,
    ports: Vec<PortInfo>,
    scanning: bool,
    notice: Option<(&'static str, String)>,
    format: Format,
    ending: Ending,
    history: VecDeque<DraftMessage>,
    history_index: Option<usize>,
    before_history: Option<DraftMessage>,
    repeat: Option<Repeat>,
}

impl App {
    pub fn new(lang: Language, port: Option<String>, baud: Option<u32>) -> std::io::Result<Self> {
        let settings_path = settings_store::path();
        let (mut draft, load_error) = match settings_path
            .as_deref()
            .map(settings_store::load)
            .transpose()
        {
            Ok(settings) => (settings.unwrap_or_default(), None),
            Err(error) => (Settings::default(), Some(error.to_string())),
        };
        if let Some(port) = port {
            draft.port = port;
        }
        if let Some(baud) = baud {
            draft.baud = baud;
        }
        let mut app = Self {
            worker: Worker::start()?,
            settings_path,
            capture: None,
            lang,
            draft,
            active: None,
            pending: false,
            first_open: FirstOpen::NotYet,
            mode: Mode::Both,
            pane: Pane::Settings,
            setting: 0,
            option: 0,
            modal: None,
            input: Editor::default(),
            log: Log::default(),
            display: Format::Text,
            search: None,
            ports: Vec::new(),
            scanning: false,
            notice: None,
            format: Format::Text,
            ending: Ending::CrLf,
            history: VecDeque::new(),
            history_index: None,
            before_history: None,
            repeat: None,
        };
        if let Some(error) = load_error {
            app.record_system("settings_load_failed", error.clone());
            app.notice("settings_load_failed", error);
        }
        app.refresh_ports();
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

    fn refresh_ports(&mut self) {
        if !self.scanning {
            self.scanning = self.enqueue(Command::Scan);
        }
    }

    pub fn port_scan_message(&self) -> String {
        if self.scanning {
            return self.lang.text("scanning").into();
        }
        match self.notice {
            Some(("scan_failed" | "busy" | "worker_down", _)) => self.message(),
            _ => String::new(),
        }
    }

    fn switch_language(&mut self) {
        self.lang = match self.lang {
            Language::En => Language::Zh,
            Language::Zh => Language::En,
        };
        if let Some(search) = &mut self.search {
            search.clear_hit();
        }
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
        self.stop_repeat();
        self.worker.invalidate_writes();
        self.pending = self.enqueue(Command::Open(self.draft.clone()));
        if self.pending && self.first_open == FirstOpen::NotYet {
            self.first_open = FirstOpen::FocusOnSuccess;
        }
    }

    fn select_port(&mut self, path: &str) {
        self.draft.port = path.into();
        self.setting = CONNECT_ROW;
    }

    fn disconnect(&mut self) {
        if self.pending || self.active.is_none() {
            return;
        }
        self.stop_repeat();
        self.worker.invalidate_writes();
        self.pending = self.enqueue(Command::Close);
    }

    pub fn settings_rows(&self, compact: bool) -> [(String, String); 9] {
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
                    t(if self.active.is_none() {
                        "connect"
                    } else if compact {
                        "reconnect"
                    } else {
                        "apply"
                    })
                ),
                String::new(),
            ),
            (format!("[ {} ]", t("scan")), String::new()),
            (format!("[ {} ]", t("disconnect")), String::new()),
        ]
    }

    pub fn options_rows(&self, compact: bool) -> [String; 4] {
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
            format!("F7 {}", t("presets")),
            format!(
                "F10 {} {}",
                if compact {
                    self.lang.pick("Repeat", "循环")
                } else {
                    t("repeat")
                },
                self.repeat.as_ref().map_or_else(
                    || t("off").into(),
                    |repeat| format!("{} ms", repeat.interval.as_millis())
                )
            ),
        ]
    }

    pub fn send_hint(&self) -> Option<String> {
        if self.pending {
            return Some(self.lang.text("pending").into());
        }
        if self.active.is_none() {
            return Some(
                self.lang
                    .pick("Connect first · F2 Ports", "请先连接串口 · F2 设置")
                    .into(),
            );
        }
        if let Some(repeat) = &self.repeat {
            let ms = repeat.interval.as_millis();
            return Some(match self.lang {
                Language::En => format!("Every {ms} ms · F10 Stop"),
                Language::Zh => format!("每 {ms} ms 发送 · F10 停止"),
            });
        }
        (self.pane == Pane::Input).then(|| {
            if self.input.value.is_empty() {
                self.lang
                    .pick("Type a command · Enter Send", "输入指令 · Enter 发送")
                    .into()
            } else {
                format!("Enter {}", self.lang.text("send_now"))
            }
        })
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
                self.modal = Some(Modal::Ports(PortMenu::new(
                    self.ports.clone(),
                    &self.draft.port,
                )));
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
            CONNECT_ROW => self.connect(),
            7 => self.refresh_ports(),
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
            3 if self.repeat.is_some() => self.stop_repeat(),
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
                    EditField::Capture => Local::now()
                        .format("serial-capture-%Y%m%d-%H%M%S.log")
                        .to_string(),
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
                self.select_port(value);
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
            EditField::Capture => match Capture::start(PathBuf::from(value)) {
                Ok(capture) => {
                    self.capture = Some(capture);
                    self.notice("capture_started", value.into());
                    None
                }
                Err(error) => Some(("capture_failed", error.to_string())),
            },
        };
        if let Some((key, detail)) = problem {
            self.notice(key, detail);
            self.modal = Some(Modal::Edit(field, editor));
        }
    }

    fn modal_key(&mut self, event: KeyEvent, log_height: usize, port_page: usize) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        match &mut modal {
            _ if event.code == KeyCode::Esc => return,
            Modal::Ports(menu) => {
                let last = menu.manual_index();
                menu.offset = menu.visible(port_page).start;
                match event.code {
                    KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
                    KeyCode::Down => menu.selected = (menu.selected + 1).min(last),
                    KeyCode::PageUp => menu.selected = menu.selected.saturating_sub(port_page),
                    KeyCode::PageDown => menu.selected = (menu.selected + port_page).min(last),
                    KeyCode::Home => menu.selected = 0,
                    KeyCode::End => menu.selected = last,
                    KeyCode::F(5) => self.refresh_ports(),
                    KeyCode::F(6) => self.switch_language(),
                    KeyCode::Enter => {
                        if let Some(path) = menu.selected_path() {
                            self.select_port(path);
                        } else {
                            self.apply(Value::Edit(EditField::Port));
                        }
                        return;
                    }
                    _ => {}
                }
            }
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
            Modal::Search(input) => match event.code {
                KeyCode::Tab => input.hex = !input.hex,
                KeyCode::F(3) => input.rx_only = !input.rx_only,
                KeyCode::Enter => match input.compile() {
                    Ok(search) => {
                        self.search = Some(search);
                        self.jump_to_match(log_height, true);
                        return;
                    }
                    Err(key) => self.notice(key, String::new()),
                },
                _ => input.editor.key(event),
            },
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
            self.notice("connect_first", String::new());
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
        if let Err(key) = self.worker.write(bytes.clone(), interval.is_some()) {
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

    fn stop_repeat(&mut self) {
        if self.repeat.take().is_some() {
            self.worker.cancel_repeat_writes();
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
            self.stop_repeat();
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
        match &mut self.modal {
            Some(Modal::Edit(_, editor)) => editor.insert(input),
            Some(Modal::Search(search)) => search.editor.insert(input),
            None if self.pane == Pane::Input => {
                self.input.insert(input);
                self.history_index = None;
            }
            _ => {}
        }
    }

    fn jump_to_match(&mut self, height: usize, reverse: bool) {
        let Some(search) = &mut self.search else {
            self.notice("search_empty", String::new());
            return;
        };
        if search.jump(&mut self.log, self.mode, self.lang, height, reverse) {
            self.display = search.format();
        } else {
            self.notice("search_not_found", String::new());
        }
    }

    pub fn search_label(&self) -> Option<String> {
        let search = self.search.as_ref()?;
        let scope = if search.rx_only { " RX" } else { "" };
        Some(format!("/{}{}", search.input, scope))
    }

    pub fn search_highlight(&self, entry: &LogEntry) -> Option<Range<usize>> {
        self.search.as_ref()?.highlight(entry, self.display)
    }

    // Returns true when the application should exit.
    pub fn key(&mut self, event: KeyEvent, log_height: usize, port_page: usize) -> bool {
        if matches!(event.code, KeyCode::Char('q' | 'c'))
            && event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return true;
        }
        self.notice = None;
        if self.modal.is_some() {
            self.modal_key(event, log_height, port_page);
            return false;
        }
        match event.code {
            KeyCode::F(2) => self.pane = Pane::Settings,
            KeyCode::F(3) => self.switch_mode(),
            KeyCode::F(4) => self.log.toggle_follow(),
            KeyCode::F(6) => self.switch_language(),
            KeyCode::F(7 | 10) if self.mode != Mode::Rx => {
                self.pane = Pane::Options;
                self.option = if event.code == KeyCode::F(7) { 2 } else { 3 };
                self.activate_option();
            }
            KeyCode::F(8) => self.apply(Value::Edit(EditField::Export)),
            KeyCode::F(9) => {
                if let Some(capture) = &mut self.capture {
                    capture.stop();
                } else {
                    self.apply(Value::Edit(EditField::Capture));
                }
            }
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
                    KeyCode::End => self.log.follow(),
                    KeyCode::Left => self.log.horizontal = self.log.horizontal.saturating_sub(8),
                    KeyCode::Right => self.log.horizontal = (self.log.horizontal + 8).min(5000),
                    KeyCode::Char('/') => {
                        self.modal = Some(Modal::Search(SearchInput::new(
                            self.search.as_ref(),
                            self.display,
                        )));
                    }
                    KeyCode::Char('n') => self.jump_to_match(log_height, false),
                    KeyCode::Char('N') => self.jump_to_match(log_height, true),
                    KeyCode::Esc => self.search = None,
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
        if self.first_open == FirstOpen::FocusOnSuccess
            && (self.pane != Pane::Settings || self.setting != CONNECT_ROW || self.modal.is_some())
        {
            self.first_open = FirstOpen::NotYet;
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
                    self.scanning = false;
                    self.active = None;
                    self.pending = false;
                    self.stop_repeat();
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
            if let Err(key) = self.worker.write(repeat.bytes.clone(), true) {
                self.stop_repeat();
                self.notice(key, String::new());
                changed = true;
            }
        }
        if let Some(capture) = self.capture.take_if(|capture| capture.is_finished()) {
            let (path, result) = capture.finish();
            let path = path.display().to_string();
            let (key, detail) = match result {
                Ok(false) => ("capture_saved", path),
                Ok(true) => ("capture_overflow", path),
                Err(error) => ("capture_failed", format!("{path} · {error}")),
            };
            self.record_system(key, detail.clone());
            self.notice(key, detail);
            changed = true;
        }
        changed
    }

    pub fn capture_label(&self) -> Option<&str> {
        self.capture.as_ref().map(|capture| {
            if capture.is_recording() {
                self.lang.pick("● REC", "● 录制中")
            } else {
                self.lang.pick("Saving…", "保存中…")
            }
        })
    }

    pub fn finish_capture_on_exit(mut self) -> std::io::Result<()> {
        if self.capture.is_some() {
            self.worker.stop();
            if let Some(events) = self.worker.events.take() {
                for event in events {
                    self.serial_event(event);
                }
            }
        }
        if let Some(capture) = self.capture.take() {
            let (path, result) = capture.finish();
            let label = match result {
                Ok(false) => return Ok(()),
                Ok(true) => self.lang.text("capture_overflow").to_owned(),
                Err(error) => format!("{} · {error}", self.lang.text("capture_failed")),
            };
            return Err(std::io::Error::other(format!(
                "{label} · {}",
                path.display()
            )));
        }
        Ok(())
    }

    fn record_system(&mut self, key: &'static str, detail: String) {
        if let Some(capture) = self
            .capture
            .as_mut()
            .filter(|capture| capture.is_recording())
        {
            capture.push(log_line(
                &Local::now(),
                Direction::System,
                &system_text(self.lang, key, &detail),
            ));
        }
        self.log.system(key, detail);
    }

    fn capture_bytes(&mut self, direction: Direction, bytes: &[u8]) {
        if let Some(capture) = self
            .capture
            .as_mut()
            .filter(|capture| capture.is_recording())
        {
            capture.push(log_line(&Local::now(), direction, &hex(bytes)));
        }
    }

    fn record_port_failure(&mut self, error: &PortFailure, fallback: &'static str) -> &'static str {
        let key = match error.kind {
            PortFailureKind::Permission => "port_permission",
            PortFailureKind::Busy => "port_busy",
            PortFailureKind::Missing => "port_missing",
            PortFailureKind::Settings => "port_settings",
            PortFailureKind::Disconnected => "port_disconnected",
            PortFailureKind::Other => fallback,
        };
        self.record_system(key, format!("{} · {}", error.path, error.detail));
        key
    }

    fn serial_event(&mut self, event: Event) {
        match event {
            Event::Ports(ports) => {
                self.scanning = false;
                self.notice("found", ports.len().to_string());
                if let Some(Modal::Ports(menu)) = &mut self.modal {
                    menu.refresh(ports.clone(), &self.draft.port);
                }
                self.ports = ports;
            }
            Event::Opened(settings) => {
                if self.first_open == FirstOpen::FocusOnSuccess {
                    self.pane = match self.mode {
                        Mode::Both | Mode::Rx => Pane::Log,
                        Mode::Tx => Pane::Input,
                    };
                }
                self.first_open = FirstOpen::Complete;
                self.record_system("opened", settings.summary());
                self.pending = false;
                self.notice = None;
                if let Some(path) = &self.settings_path
                    && let Err(error) = settings_store::save(path, &settings)
                {
                    let detail = error.to_string();
                    self.record_system("settings_save_failed", detail.clone());
                    self.notice("settings_save_failed", detail);
                }
                self.active = Some(settings);
            }
            Event::Closed => {
                self.record_system("closed", String::new());
                self.active = None;
                self.pending = false;
            }
            Event::OpenFailed {
                error,
                retained,
                restore_error,
            } => {
                self.pending = false;
                if self.first_open == FirstOpen::FocusOnSuccess {
                    self.first_open = FirstOpen::NotYet;
                }
                let key = self.record_port_failure(&error, "open_failed");
                if let Some(settings) = &retained {
                    self.record_system("connection_kept", settings.summary());
                    let still_connected = self.lang.text("still_connected");
                    self.notice(
                        key,
                        format!("{} · {still_connected} {}", error.path, settings.port),
                    );
                } else if let Some(restore_error) = restore_error {
                    self.record_system("restore_failed", restore_error.path.clone());
                    self.record_port_failure(&restore_error, "open_failed");
                    self.notice("restore_failed", restore_error.path);
                } else {
                    self.notice(key, error.path);
                }
                self.active = retained;
            }
            Event::Lost(error) => {
                self.active = None;
                self.stop_repeat();
                let key = self.record_port_failure(&error, "lost");
                self.notice(key, error.path);
            }
            Event::ScanFailed(error) => {
                self.scanning = false;
                self.notice("scan_failed", error);
            }
            Event::WriteFailed(error) => {
                self.stop_repeat();
                self.record_system("write_failed", error.clone());
                self.notice("write_failed", error);
            }
            Event::Rx(bytes) => {
                self.capture_bytes(Direction::Rx, &bytes);
                self.log.receive(&bytes);
            }
            Event::Tx(bytes) => {
                self.capture_bytes(Direction::Tx, &bytes);
                self.log.sent(bytes);
            }
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
    fn first_connection_focus_respects_work_mode_and_manual_navigation() {
        let new_app = |mode| {
            let mut app =
                App::new(Language::En, Some("/dev/serial-tui-test".into()), None).unwrap();
            app.settings_path = None;
            app.mode = mode;
            app.setting = CONNECT_ROW;
            app
        };
        let press = |app: &mut App, key| {
            app.key(KeyEvent::new(key, KeyModifiers::NONE), 10, 8);
        };

        for (mode, pane) in [
            (Mode::Both, Pane::Log),
            (Mode::Rx, Pane::Log),
            (Mode::Tx, Pane::Input),
        ] {
            let mut app = new_app(mode);
            press(&mut app, KeyCode::Enter);
            app.serial_event(Event::Opened(app.draft.clone()));
            assert!(app.pane == pane, "first open in {}", mode.label());

            press(&mut app, KeyCode::F(2));
            press(&mut app, KeyCode::Enter);
            app.serial_event(Event::Opened(app.draft.clone()));
            assert!(app.pane == Pane::Settings, "reconnect in {}", mode.label());
        }

        let mut app = new_app(Mode::Both);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::F(2));
        app.serial_event(Event::Opened(app.draft.clone()));
        assert!(app.pane == Pane::Settings);
    }

    #[test]
    fn port_popup_pages_and_keeps_the_selected_device_when_refresh_changes_its_path() {
        use crate::{serial::PortKind, ui};
        use ratatui::{Terminal, backend::TestBackend, layout::Rect};

        let mut app = App::new(Language::En, Some("/dev/ttyS0".into()), None).unwrap();
        let mut ports: Vec<_> = (0..14)
            .map(|index| {
                let path = format!("/dev/ttyS{index}");
                PortInfo {
                    node: path.clone(),
                    path,
                    kind: PortKind::OnboardUart,
                }
            })
            .collect();
        app.serial_event(Event::Ports(ports.clone()));
        let screen = Rect::new(0, 0, 64, 17);
        let key = |app: &mut App, code| {
            app.key(
                KeyEvent::new(code, KeyModifiers::NONE),
                ui::log_height(screen, app.mode),
                ui::port_page_size(screen, app),
            );
        };
        key(&mut app, KeyCode::Enter);
        let page = ui::port_page_size(screen, &app);
        key(&mut app, KeyCode::PageDown);
        let node = ports[page].path.clone();
        let alias = "/dev/serial/by-id/usb-FTDI_abcdefghijklmno0123456789-if00-port0";
        key(&mut app, KeyCode::F(5));
        assert_eq!(app.port_scan_message(), "Refreshing…");
        ports[page].path = alias.into();
        ports[page].kind = PortKind::Usb;
        ports.remove(0);
        app.serial_event(Event::Ports(ports.clone()));
        app.active = Some(Settings {
            port: node.clone(),
            ..Settings::default()
        });

        let Some(Modal::Ports(menu)) = &app.modal else {
            panic!("refresh should keep the popup open");
        };
        assert_eq!(menu.selected_path(), Some(alias));
        assert_eq!(menu.selected, page - 1);
        let visible = menu.visible(ui::port_page_size(screen, &app));
        assert!(visible.contains(&menu.selected) && visible.start > 0);

        for (title, usb, connected, refresh) in [
            ("Port · 13 devices", "USB serial", "Connected", "F5 Refresh"),
            ("串 口", "USB 转 串 口", "当 前 连 接", "F5 刷 新"),
        ] {
            let mut terminal =
                Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();
            terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
            let output: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            for text in [title, usb, connected, refresh, &node, "port0", "PgUp/PgDn"] {
                assert!(output.contains(text), "{text}: {output}");
            }
            key(&mut app, KeyCode::F(6));
        }

        key(&mut app, KeyCode::F(5));
        app.serial_event(Event::ScanFailed("fixture error".into()));
        assert_eq!(
            app.port_scan_message(),
            "Could not list ports · fixture error"
        );
        let Some(Modal::Ports(menu)) = &app.modal else {
            panic!("failed refresh should keep the popup open");
        };
        assert_eq!(menu.selected_path(), Some(alias));
        let mut selected = ports.remove(page - 1);
        selected.path = "/dev/serial/by-path/usb-connector".into();
        ports.push(selected);
        app.serial_event(Event::Ports(ports.clone()));
        let Some(Modal::Ports(menu)) = &app.modal else {
            panic!("alias fallback should keep the popup open");
        };
        assert_eq!(
            menu.selected_path(),
            Some("/dev/serial/by-path/usb-connector")
        );
        ports.retain(|port| port.node != node);
        let last = ports.len() - 1;
        app.serial_event(Event::Ports(ports));
        let Some(Modal::Ports(menu)) = &app.modal else {
            panic!("device removal should keep the popup open");
        };
        assert_eq!(menu.selected, last);
        key(&mut app, KeyCode::End);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal, Some(Modal::Edit(EditField::Port, _))));
    }

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
    fn receive_preserves_utf8_at_the_record_boundary_across_reads() {
        for character in ['串', '🦀'] {
            let mut bytes = vec![b'A'; 511];
            bytes.extend_from_slice(format!("{character}尾\n").as_bytes());
            for split in 511..=515 {
                let mut log = Log::default();
                log.receive(&bytes[..split]);
                log.receive(&bytes[split..]);
                let rows = log.visible(Mode::Rx, 10);
                assert!(
                    rows.iter()
                        .all(|row| std::str::from_utf8(&row.data).is_ok())
                );
                assert!(
                    rows[0]
                        .body(Language::Zh, Format::Text)
                        .ends_with(character)
                );
                assert_eq!(
                    rows.iter()
                        .flat_map(|row| row.data.iter().copied())
                        .collect::<Vec<_>>(),
                    bytes
                );
            }
        }
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
        assert_eq!(log.visible(Mode::Rx, 10)[0].data, b"beforeafter\n");
    }

    #[test]
    fn paused_rows_remain_visible_after_the_live_buffer_wraps() {
        let mut log = Log::default();
        log.receive(b"first\nlast visible\n");
        log.toggle_follow();
        log.receive(&b"new\n".repeat(2100));
        assert_eq!(log.visible(Mode::Rx, 1)[0].data, b"last visible\n");
        assert_eq!(log.entries.len(), 2000);

        log.scroll(Mode::Rx, 1, -1);
        assert_eq!(log.visible(Mode::Rx, 1)[0].data, b"first\n");
        log.follow();
        assert_eq!(log.visible(Mode::Rx, 1)[0].data, b"new\n");
    }
}
