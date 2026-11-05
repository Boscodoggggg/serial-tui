use std::ops::Range;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        App, Direction, Editor, Language, LogEntry, LogViewport, Modal, Mode, Pane, PortMenu,
        clean_text, pan_byte,
    },
    serial::{PortKind, same_device},
};

const BACKGROUND: Color = Color::Rgb(16, 22, 29);
const PANEL: Color = Color::Rgb(19, 28, 36);
const BAR: Color = Color::Rgb(27, 40, 51);
const TEXT: Color = Color::Rgb(220, 231, 240);
const DIM: Color = Color::Rgb(157, 177, 192);
const EDGE: Color = Color::Rgb(82, 107, 125);
const MINT: Color = Color::Rgb(128, 225, 206);
const AMBER: Color = Color::Rgb(247, 200, 132);
const LOG_PREFIX_WIDTH: u16 = 18;
const FOCUS: Style = Style::new()
    .fg(Color::Rgb(17, 47, 42))
    .bg(MINT)
    .add_modifier(Modifier::BOLD);

struct Areas {
    header: Rect,
    modes: Rect,
    settings: Rect,
    log: Rect,
    send: Rect,
    notice: Rect,
    help: Rect,
    footer: Rect,
}

impl Areas {
    fn new(area: Rect, mode: Mode) -> Option<Self> {
        if area.width < 64 || area.height < 17 {
            return None;
        }
        let [header, modes, body, notice, help, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(11),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        let side = if area.width >= 90 { 30 } else { 25 };
        let [settings, right] = Layout::horizontal([Constraint::Length(side), Constraint::Min(1)])
            .spacing(1)
            .areas(body);
        let (log, send) = if mode == Mode::Rx {
            (right, Rect::default())
        } else {
            let [log, send] =
                Layout::vertical([Constraint::Min(6), Constraint::Length(5)]).areas(right);
            (log, send)
        };
        Some(Self {
            header,
            modes,
            settings,
            log,
            send,
            notice,
            help,
            footer,
        })
    }
}

pub fn log_viewport(area: Rect, mode: Mode) -> LogViewport {
    let log = Areas::new(area, mode).map_or(Rect::default(), |areas| areas.log);
    LogViewport {
        height: usize::from(log.height.saturating_sub(3).max(1)),
        width: usize::from(log.width.saturating_sub(4 + LOG_PREFIX_WIDTH).max(1)),
    }
}

fn panel(title: &str, focused: bool, bg: Color) -> Block<'_> {
    Block::bordered()
        .title(Line::styled(
            format!(" {title} "),
            Style::new().fg(if focused { MINT } else { TEXT }).bold(),
        ))
        .border_style(Style::new().fg(if focused { MINT } else { EDGE }))
        .style(Style::new().fg(TEXT).bg(bg))
        .padding(Padding::horizontal(1))
}

pub fn draw(frame: &mut Frame, app: &App) {
    let screen = frame.area();
    frame.render_widget(
        Block::default().style(Style::new().fg(TEXT).bg(BACKGROUND)),
        screen,
    );
    let Some(areas) = Areas::new(screen, app.mode) else {
        frame.render_widget(
            Paragraph::new(app.lang.text("min_size")).wrap(ratatui::widgets::Wrap { trim: false }),
            screen,
        );
        return;
    };
    draw_header(frame, app, &areas);
    draw_settings(frame, app, areas.settings);
    draw_log(frame, app, areas.log);
    if app.mode != Mode::Rx {
        draw_send(frame, app, areas.send);
    }
    let notice = app.message();
    frame.render_widget(
        Line::styled(notice.as_str(), Style::new().fg(AMBER)),
        areas.notice,
    );
    draw_keys(frame, app, &areas);
    if let Some(modal) = &app.modal {
        draw_modal(frame, app, modal, screen, &notice);
    }
}

fn draw_header(frame: &mut Frame, app: &App, areas: &Areas) {
    let t = |key| app.lang.text(key);
    let title = app.capture_label().map_or_else(
        || format!(" {}", t("title")),
        |label| format!(" {}  {label}", t("title")),
    );
    let status = if app.pending {
        t("pending").into()
    } else if let Some(active) = &app.active {
        clean_text(format!("{}  {}", t("online"), active.summary()))
    } else {
        t("offline").into()
    };
    let [left, right] = Layout::horizontal([
        Constraint::Length(title.width() as u16 + 2),
        Constraint::Min(1),
    ])
    .areas(areas.header);
    frame.render_widget(
        Line::styled(title, Style::new().fg(MINT).bg(BAR).bold()),
        left,
    );
    let color = if app.active.is_some() && !app.pending {
        MINT
    } else {
        DIM
    };
    let status = truncate(&status, right.width.saturating_sub(1) as usize);
    frame.render_widget(
        Line::styled(format!("{status} "), Style::new().fg(color).bg(BAR))
            .alignment(Alignment::Right),
        right,
    );

    let mut modes = vec![Span::styled(
        format!(" {}  ", t("mode")),
        Style::new().fg(DIM),
    )];
    for mode in Mode::ALL {
        let style = if mode == app.mode {
            FOCUS
        } else {
            Style::new().fg(TEXT).bg(PANEL)
        };
        modes.push(Span::styled(format!(" {} ", t(mode.label())), style));
        modes.push(Span::raw("  "));
    }
    modes.push(Span::styled(
        app.lang.pick("EN / 中文", "中文 / EN"),
        Style::new().fg(DIM),
    ));
    frame.render_widget(Line::from(modes), areas.modes);
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.pane == Pane::Settings;
    let block = panel(app.lang.text("settings"), focused, PANEL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = app.settings_rows(inner.width < 23);
    let label_column = rows[..6]
        .iter()
        .map(|(label, _)| label.width())
        .max()
        .unwrap_or(0)
        + 1;
    for (index, (label, value)) in rows.iter().enumerate() {
        let extra_gap = u16::from(index >= 6 && inner.height >= 10);
        let row = index as u16 + extra_gap;
        if row >= inner.height {
            break;
        }
        let selected = focused && app.setting == index;
        let mut spans = vec![Span::styled(
            if selected { "› " } else { "  " },
            if selected { FOCUS } else { Style::new() },
        )];
        if value.is_empty() {
            spans.push(Span::raw(label.as_str()));
        } else {
            spans.push(Span::styled(
                label.as_str(),
                if selected {
                    FOCUS
                } else {
                    Style::new().fg(DIM)
                },
            ));
            let value_column = if index == 0 {
                label_column.min(label.width() + 4)
            } else {
                label_column
            };
            spans.push(Span::raw(" ".repeat(value_column - label.width())));
            spans.push(Span::raw(truncate(
                value,
                usize::from(inner.width).saturating_sub(value_column + 2),
            )));
        }
        let line = Line::from(spans).style(if selected {
            FOCUS
        } else {
            Style::new().fg(TEXT)
        });
        frame.render_widget(line, Rect::new(inner.x, inner.y + row, inner.width, 1));
    }
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let name = app.lang.text(app.mode.log_title());
    let title = app.log.new_since_pause().map_or_else(
        || name.to_owned(),
        |count| {
            let paused = app.lang.text("paused");
            let status = match app.lang {
                Language::En => format!("{paused} · {count} new"),
                Language::Zh => format!("{paused} · 新增 {count} 条"),
            };
            let title = format!("{name} · {status}");
            if title.width() + 4 <= usize::from(area.width) {
                title
            } else {
                status
            }
        },
    );
    let block = panel(&title, app.pane == Pane::Log, BACKGROUND);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let counters = format!("RX {} B / TX {} B", app.log.rx, app.log.tx);
    let [format_area, count_area] = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(counters.len() as u16),
    ])
    .areas(Rect::new(inner.x, inner.y, inner.width, 1));
    let mut format = app.lang.text(app.display.label()).to_owned();
    if app.log.wrap {
        format.push_str(" · ");
        format.push_str(app.lang.text("wrap"));
    }
    if !app.log.is_paused() {
        format.push_str(" · ");
        format.push_str(app.lang.text("following"));
    }
    if let Some(search) = app.search_label() {
        format.push_str(" · ");
        format.push_str(&search);
    }
    frame.render_widget(Line::styled(format, Style::new().fg(DIM)), format_area);
    frame.render_widget(Line::styled(counters, Style::new().fg(DIM)), count_area);

    let viewport = LogViewport {
        height: usize::from(inner.height.saturating_sub(1)),
        width: usize::from(inner.width.saturating_sub(LOG_PREFIX_WIDTH).max(1)),
    };
    let mut y = inner.y + 1;
    let mut draw_row = |entry: &LogEntry, body: &str, range: Range<usize>| {
        draw_log_row(
            frame,
            app,
            entry,
            body,
            range,
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    };
    if app.log.wrap {
        for record in app.log.wrapped(app.mode, app.lang, app.display, viewport) {
            for range in record.rows {
                draw_row(record.entry, &record.body, range);
            }
        }
    } else {
        for entry in app.log.visible(app.mode, viewport.height) {
            let body = entry.body(app.lang, app.display);
            let offset = pan_byte(&body, app.log.horizontal);
            draw_row(entry, &body, offset..body.len());
        }
    }
    if y == inner.y + 1 {
        frame.render_widget(
            Line::styled(app.lang.text("empty_log"), Style::new().fg(DIM)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}

fn draw_log_row(
    frame: &mut Frame,
    app: &App,
    entry: &LogEntry,
    body: &str,
    row: Range<usize>,
    area: Rect,
) {
    let mut spans = if !app.log.wrap || row.start == 0 {
        let color = match entry.direction {
            Direction::System => DIM,
            Direction::Rx => MINT,
            Direction::Tx => AMBER,
        };
        vec![
            Span::styled(
                entry.time.format("%H:%M:%S%.3f").to_string(),
                Style::new().fg(DIM),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{:^4}", entry.direction.label()),
                Style::new().fg(color).bold(),
            ),
            Span::raw(" "),
        ]
    } else {
        vec![Span::raw(" ".repeat(usize::from(LOG_PREFIX_WIDTH)))]
    };
    let hit = app.search_highlight(entry).and_then(|range| {
        let start = range.start.max(row.start);
        let end = range.end.min(row.end);
        if start >= end {
            return None;
        }
        Some((
            body.get(row.start..start)?,
            body.get(start..end)?,
            body.get(end..row.end)?,
        ))
    });
    if let Some((before, matched, after)) = hit {
        spans.extend([
            Span::raw(before),
            Span::styled(matched, FOCUS),
            Span::raw(after),
        ]);
    } else {
        spans.push(Span::raw(&body[row]));
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_send(frame: &mut Frame, app: &App, area: Rect) {
    let mut block = panel(
        app.lang.text("send"),
        matches!(app.pane, Pane::Options | Pane::Input),
        BACKGROUND,
    );
    if let Some(hint) = app.send_hint() {
        block = block.title_bottom(Line::styled(format!(" {hint} "), Style::new().fg(MINT)));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let options = app.options_rows(inner.width < 36);
    let first = options[..2]
        .iter()
        .enumerate()
        .flat_map(|(index, text)| option_spans(app, index, text))
        .collect::<Vec<_>>();
    let second = options[2..]
        .iter()
        .enumerate()
        .flat_map(|(index, text)| option_spans(app, index + 2, text))
        .collect::<Vec<_>>();
    frame.render_widget(
        Line::from(first),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Line::from(second),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    draw_editor(
        frame,
        &app.input,
        app.lang.text("type"),
        Rect::new(inner.x, inner.y + 2, inner.width, 1),
        app.pane == Pane::Input && app.modal.is_none(),
    );
}

fn option_spans<'a>(app: &App, index: usize, text: &'a str) -> [Span<'a>; 2] {
    let style = if app.pane == Pane::Options && app.option == index {
        FOCUS
    } else {
        Style::new().fg(DIM)
    };
    [Span::styled(text, style), Span::raw("  ")]
}

fn draw_editor(frame: &mut Frame, editor: &Editor, hint: &str, area: Rect, focused: bool) {
    let width = area.width.saturating_sub(2) as usize;
    let (input, column) = if focused {
        let before = &editor.value[..editor.cursor];
        let mut visible = 0;
        let mut start = editor.cursor;
        for (index, c) in before.char_indices().rev() {
            let next = visible + c.width().unwrap_or(0);
            if next >= width {
                break;
            }
            start = index;
            visible = next;
        }
        (&editor.value[start..], visible)
    } else {
        (editor.value.as_str(), 0)
    };
    let line = Line::from(vec![
        Span::styled("> ", Style::new().fg(MINT)),
        Span::styled(
            if input.is_empty() && !focused {
                hint
            } else {
                input
            },
            Style::new().fg(if input.is_empty() { DIM } else { TEXT }),
        ),
    ])
    .style(Style::new().bg(PANEL));
    frame.render_widget(line, area);
    if focused && width > 0 {
        frame.set_cursor_position(Position::new(area.x + 2 + column as u16, area.y));
    }
}

fn draw_keys(frame: &mut Frame, app: &App, areas: &Areas) {
    let t = |key| app.lang.text(key);
    let mut local = vec![("Tab/⇧Tab", app.lang.pick("Pane", t("switch")))];
    match app.pane {
        Pane::Settings => local.extend([("↑↓", t("select")), ("Enter", t("edit"))]),
        Pane::Log => {
            local.extend([("/", t("search")), ("W", t("wrap")), ("End", t("latest"))]);
            if app.search_label().is_some() {
                local.extend([("n/N", t("search_jump")), ("Esc", t("search_clear"))]);
            } else {
                local.extend([("PgUp/Dn", t("page")), ("H", t("view"))]);
                if !app.log.wrap {
                    local.push(("←→", t("pan")));
                }
            }
        }
        Pane::Options => local.extend([("←→", t("select")), ("Enter", t("edit"))]),
        Pane::Input => local.extend([
            ("↑↓", t("history")),
            ("Enter", t("send_now")),
            ("^U", app.lang.pick("Clear", "清空")),
        ]),
    }
    frame.render_widget(key_line(local, BACKGROUND), areas.help);
    let compact = areas.footer.width < 84;
    let global = vec![
        (
            "F2",
            app.lang
                .pick("Ports", if compact { "串口" } else { "串口设置" }),
        ),
        (
            "F3",
            app.lang
                .pick("Mode", if compact { "模式" } else { "切换模式" }),
        ),
        (
            "F4",
            app.lang.pick(
                if compact { "Hold" } else { "Pause" },
                if compact { "暂停" } else { "暂停/跟随" },
            ),
        ),
        ("F6", "中/EN"),
        (
            "F8",
            app.lang.pick(
                if compact { "Save" } else { "Export" },
                if compact { "导出" } else { "导出日志" },
            ),
        ),
        (
            "F9",
            app.lang.pick(
                if compact { "Rec" } else { "Record" },
                if compact { "录制" } else { "连续录制" },
            ),
        ),
        ("^Q", t("quit")),
    ];
    frame.render_widget(key_line(global, BAR), areas.footer);
}

fn key_line<'a>(keys: Vec<(&'a str, &'a str)>, bg: Color) -> Line<'a> {
    let mut spans = Vec::new();
    for (key, label) in keys {
        spans.push(Span::styled(key, Style::new().fg(MINT).bold()));
        spans.push(Span::styled(format!(" {label}  "), Style::new().fg(DIM)));
    }
    Line::from(spans).style(Style::new().bg(bg))
}

fn draw_modal(frame: &mut Frame, app: &App, modal: &Modal, screen: Rect, notice: &str) {
    let (title, desired_height) = match modal {
        Modal::Ports(menu) => return draw_port_menu(frame, app, menu, screen),
        Modal::Menu(menu) => (menu.title, menu.labels().count().min(12) as u16 + 3),
        Modal::Edit(field, _) => (field.label(), 5),
        Modal::Search(_) => ("search_log", 6),
    };
    let width = screen.width.min(if matches!(modal, Modal::Search(_)) {
        61
    } else {
        55
    });
    let height = (screen.height - 2).min(desired_height);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = panel(app.lang.text(title), true, PANEL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let help = match modal {
        Modal::Ports(_) => unreachable!(),
        Modal::Menu(menu) => {
            let capacity = inner.height.saturating_sub(1) as usize;
            let offset = menu.selected.saturating_sub(capacity.saturating_sub(1));
            for (index, value) in menu.labels().enumerate().skip(offset).take(capacity) {
                let selected = index == menu.selected;
                let line = format!(
                    "{}{}",
                    if selected { "› " } else { "  " },
                    truncate(value, inner.width.saturating_sub(2) as usize)
                );
                frame.render_widget(
                    Line::styled(
                        line,
                        if selected {
                            FOCUS
                        } else {
                            Style::new().fg(TEXT)
                        },
                    ),
                    Rect::new(inner.x, inner.y + (index - offset) as u16, inner.width, 1),
                );
            }
            vec![
                ("↑↓", app.lang.text("select")),
                ("Enter", app.lang.text("confirm")),
                ("Esc", app.lang.text("cancel")),
            ]
        }
        Modal::Edit(_, editor) => {
            draw_editor(
                frame,
                editor,
                "",
                Rect::new(inner.x, inner.y, inner.width, 1),
                true,
            );
            frame.render_widget(
                Line::styled(notice, Style::new().fg(AMBER)),
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
            );
            vec![
                ("Enter", app.lang.text("confirm")),
                ("Esc", app.lang.text("cancel")),
                ("^U", app.lang.pick("Clear", "清空")),
            ]
        }
        Modal::Search(search) => {
            let t = |key| app.lang.text(key);
            let format = if search.hex { "HEX" } else { t("text") };
            let scope = if search.rx_only {
                "search_rx"
            } else {
                "search_all"
            };
            frame.render_widget(
                Line::styled(format!("{format} · {}", t(scope)), Style::new().fg(DIM)),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
            draw_editor(
                frame,
                &search.editor,
                "",
                Rect::new(inner.x, inner.y + 1, inner.width, 1),
                true,
            );
            frame.render_widget(
                Line::styled(notice, Style::new().fg(AMBER)),
                Rect::new(inner.x, inner.y + 2, inner.width, 1),
            );
            vec![
                ("Tab", app.lang.pick("Text/HEX", "文本/HEX")),
                ("F3", t("search_scope")),
                ("Enter", t("confirm")),
                ("Esc", t("cancel")),
            ]
        }
    };
    frame.render_widget(
        key_line(help, PANEL),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}

struct PortPopup {
    area: Rect,
    page: usize,
    path_lines: Vec<String>,
}

impl PortPopup {
    fn new(screen: Rect, menu: &PortMenu) -> Self {
        // Two borders, a position row, and two shortcut rows.
        const FRAME_ROWS: u16 = 5;
        let width = screen.width.saturating_sub(4).min(76);
        let available = screen.height.saturating_sub(2).saturating_sub(FRAME_ROWS);
        let detail_limit = usize::from(available.saturating_sub(1));
        let mut path_lines = vec![String::new()];
        if let Some(path) = menu.selected_path() {
            let width = usize::from(width.saturating_sub(4)).max(2);
            let mut used = 0;
            for ch in clean_text(path.into()).chars() {
                let char_width = ch.width().unwrap_or(0);
                if used + char_width > width {
                    path_lines.push(String::new());
                    used = 0;
                }
                path_lines.last_mut().unwrap().push(ch);
                used += char_width;
            }
            if path_lines.len() > detail_limit && detail_limit > 0 {
                path_lines.truncate(detail_limit);
                let last = path_lines.last_mut().unwrap();
                *last = truncate(&format!("{last}…"), width);
            }
        }
        path_lines.truncate(detail_limit);
        let page = (menu.manual_index() + 1)
            .min(12)
            .min(usize::from(available).saturating_sub(path_lines.len()))
            .max(1);
        let height = (FRAME_ROWS + (page + path_lines.len()) as u16).min(screen.height);
        Self {
            area: Rect::new(
                screen.x + (screen.width - width) / 2,
                screen.y + (screen.height - height) / 2,
                width,
                height,
            ),
            page,
            path_lines,
        }
    }
}

pub fn port_page_size(screen: Rect, app: &App) -> usize {
    match &app.modal {
        Some(Modal::Ports(menu)) => PortPopup::new(screen, menu).page,
        _ => 1,
    }
}

fn draw_port_menu(frame: &mut Frame, app: &App, menu: &PortMenu, screen: Rect) {
    let popup = PortPopup::new(screen, menu);
    let t = |key| app.lang.text(key);
    let count = menu.ports.len();
    let units = app
        .lang
        .pick(if count == 1 { "device" } else { "devices" }, "个设备");
    let title = format!("{} · {count} {units}", t("port"));
    frame.render_widget(Clear, popup.area);
    let block = panel(&title, true, PANEL);
    let inner = block.inner(popup.area);
    frame.render_widget(block, popup.area);
    let row = |offset| Rect::new(inner.x, inner.y + offset, inner.width, 1);
    let active = app.active.as_ref().map(|active| active.port.as_str());
    let visible = menu.visible(popup.page);
    for (offset, index) in visible.clone().enumerate() {
        let (label, kind, connected) = if let Some(port) = menu.ports.get(index) {
            let kind = match port.kind {
                PortKind::Usb => t("usb_port"),
                PortKind::OnboardUart => t("uart_port"),
                PortKind::Pci => t("pci_port"),
                PortKind::Bluetooth => t("bluetooth_port"),
                PortKind::Unknown => "",
            };
            (
                clean_text(port.node.clone()),
                kind,
                active.is_some_and(|path| port.matches(path)),
            )
        } else if index < menu.manual_index() {
            let path = menu.configured.as_deref().unwrap_or_default();
            (
                clean_text(path.into()),
                t("configured_port"),
                active.is_some_and(|active| same_device(active, path)),
            )
        } else {
            (t("manual_port").into(), "", false)
        };
        let kind = match (kind, connected) {
            ("", true) => t("current_port").into(),
            (kind, true) => format!("{kind} · {}", t("current_port")),
            (kind, false) => kind.into(),
        };
        let selected = index == menu.selected;
        let prefix = if selected { "› " } else { "  " };
        let width = usize::from(inner.width).saturating_sub(prefix.width());
        let line = if kind.is_empty() {
            format!("{prefix}{}", truncate(&label, width))
        } else {
            let name_width = width.saturating_sub(kind.width() + 2);
            let name = truncate(&label, name_width);
            let padding = " ".repeat(name_width.saturating_sub(name.width()));
            format!("{prefix}{name}{padding}  {kind}")
        };
        let style = if selected {
            FOCUS
        } else {
            Style::new().fg(TEXT)
        };
        frame.render_widget(Line::styled(line, style), row(offset as u16));
    }

    let mut footer = popup.page as u16;
    let up = if visible.start > 0 { "↑" } else { " " };
    let down = if visible.end <= menu.manual_index() {
        "↓"
    } else {
        " "
    };
    let position = if menu.selected < count {
        (menu.selected + 1).to_string()
    } else {
        "—".into()
    };
    let status = format!(
        "{up} {position} / {count} {down}  {}",
        app.port_scan_message()
    );
    frame.render_widget(Line::styled(status, Style::new().fg(MINT)), row(footer));
    footer += 1;
    for path in popup.path_lines {
        frame.render_widget(Line::styled(path, Style::new().fg(DIM)), row(footer));
        footer += 1;
    }
    frame.render_widget(
        key_line(
            vec![
                ("↑↓", t("select")),
                ("PgUp/PgDn", t("page")),
                ("Enter", t("confirm")),
            ],
            PANEL,
        ),
        row(footer),
    );
    frame.render_widget(
        key_line(
            vec![("F5", t("refresh")), ("F6", "中/EN"), ("Esc", t("cancel"))],
            PANEL,
        ),
        row(footer + 1),
    );
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut remaining = width - 1;
    let mut prefix = String::new();
    for c in value.chars() {
        let Some(left) = remaining.checked_sub(c.width().unwrap_or(0)) else {
            break;
        };
        remaining = left;
        prefix.push(c);
    }
    prefix.push('…');
    prefix
}
