use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Format, Language, Log, LogEntry, Mode};

#[derive(Clone, Copy)]
pub struct LogViewport {
    pub height: usize,
    pub width: usize,
}

pub struct WrappedRecord<'a> {
    pub entry: &'a LogEntry,
    pub body: String,
    pub rows: Vec<Range<usize>>,
}

#[derive(Clone, Copy)]
struct WrapStyle {
    lang: Language,
    format: Format,
    width: usize,
}

impl WrapStyle {
    fn record(self, entry: &LogEntry) -> WrappedRecord<'_> {
        let body = entry.body(self.lang, self.format);
        let rows = wrap_ranges(&body, self.width);
        WrappedRecord { entry, body, rows }
    }
}

impl Log {
    pub fn wrapped(
        &self,
        mode: Mode,
        lang: Language,
        format: Format,
        viewport: LogViewport,
    ) -> Vec<WrappedRecord<'_>> {
        let style = WrapStyle {
            lang,
            format,
            width: viewport.width,
        };
        let entries = self.filtered(mode);
        let mut remaining = viewport.height;
        let mut records = Vec::new();
        for entry in entries[..self.end(&entries)].iter().rev() {
            if remaining == 0 {
                break;
            }
            let mut record = style.record(entry);
            let end = row_at(&record.rows, self.paused_byte(entry.id)) + 1;
            record.rows.truncate(end);
            record.rows.drain(..end.saturating_sub(remaining));
            remaining -= record.rows.len();
            records.push(record);
        }
        records.reverse();
        records
    }

    pub(super) fn toggle_wrap(
        &mut self,
        mode: Mode,
        lang: Language,
        format: Format,
        viewport: LogViewport,
    ) {
        self.wrap = !self.wrap;
        let Some(paused) = &mut self.paused else {
            return;
        };
        let Some(entry) = paused
            .entries
            .iter()
            .rev()
            .find(|entry| entry.id <= paused.anchor && entry.direction.visible_in(mode))
        else {
            return;
        };
        let body = entry.body(lang, format);
        if self.wrap {
            paused.anchor = entry.id;
            paused.byte = Some(wrapped_anchor(
                &body,
                viewport,
                pan_byte(&body, self.horizontal),
            ));
        } else {
            let rows = wrap_ranges(&body, viewport.width);
            let byte = paused.byte.filter(|_| entry.id == paused.anchor);
            let first = (row_at(&rows, byte) + 1).saturating_sub(viewport.height.max(1));
            self.horizontal = body[..rows[first].start].width();
        }
    }

    pub(super) fn scroll_wrapped(
        &mut self,
        mode: Mode,
        lang: Language,
        format: Format,
        viewport: LogViewport,
        delta: isize,
    ) {
        let entries = self.filtered(mode);
        if entries.is_empty() {
            if delta > 0 {
                self.follow();
            } else {
                self.pause_at(0);
            }
            return;
        }
        let style = WrapStyle {
            lang,
            format,
            width: viewport.width,
        };
        let end = self.end(&entries);
        let index = end.saturating_sub(1);
        let byte = if end == 0 {
            Some(0)
        } else {
            self.paused_byte(entries[index].id)
        };
        let mut cursor = WrappedCursor::new(&entries, style, index, byte);
        if delta > 0 && cursor.index + 1 == entries.len() && cursor.row + 1 == cursor.rows.len() {
            self.follow();
            return;
        }
        let first_page = || {
            let mut first = WrappedCursor::new(&entries, style, 0, Some(0));
            first.shift(true, viewport.height.saturating_sub(1));
            first
        };
        if delta == isize::MIN {
            cursor = first_page();
        } else {
            cursor.shift(delta > 0, delta.unsigned_abs());
            if delta < 0 && cursor.index < viewport.height {
                let first = first_page();
                if (cursor.index, cursor.row) < (first.index, first.row) {
                    cursor = first;
                }
            }
        }

        self.pause_at_byte(
            entries[cursor.index].id,
            Some(cursor.rows[cursor.row].start),
        );
    }

    fn paused_byte(&self, id: u64) -> Option<usize> {
        self.paused
            .as_ref()
            .filter(|paused| paused.anchor == id)?
            .byte
    }
}

struct WrappedCursor<'a, 'b> {
    entries: &'a [&'b LogEntry],
    style: WrapStyle,
    index: usize,
    rows: Vec<Range<usize>>,
    row: usize,
}

impl<'a, 'b> WrappedCursor<'a, 'b> {
    fn new(
        entries: &'a [&'b LogEntry],
        style: WrapStyle,
        index: usize,
        byte: Option<usize>,
    ) -> Self {
        let rows = style.record(entries[index]).rows;
        let row = row_at(&rows, byte);
        Self {
            entries,
            style,
            index,
            rows,
            row,
        }
    }

    fn shift(&mut self, forward: bool, count: usize) {
        for _ in 0..count {
            if forward && self.row + 1 < self.rows.len() {
                self.row += 1;
            } else if !forward && self.row > 0 {
                self.row -= 1;
            } else {
                let next = if forward {
                    (self.index + 1 < self.entries.len()).then_some(self.index + 1)
                } else {
                    self.index.checked_sub(1)
                };
                let Some(next) = next else { break };
                self.index = next;
                self.rows = self.style.record(self.entries[next]).rows;
                self.row = if forward { 0 } else { self.rows.len() - 1 };
            }
        }
    }
}

fn row_at(rows: &[Range<usize>], byte: Option<usize>) -> usize {
    byte.map_or(rows.len() - 1, |byte| {
        rows.partition_point(|row| row.start <= byte)
            .saturating_sub(1)
    })
}

pub(super) fn wrapped_anchor(body: &str, viewport: LogViewport, byte: usize) -> usize {
    let rows = wrap_ranges(body, viewport.width);
    let end = (row_at(&rows, Some(byte)) + viewport.height.saturating_sub(1)).min(rows.len() - 1);
    rows[end].start
}

pub(crate) fn pan_byte(body: &str, offset: usize) -> usize {
    let mut columns = 0;
    for (index, grapheme) in body.grapheme_indices(true) {
        if columns >= offset {
            return index;
        }
        columns += grapheme.width();
    }
    body.len()
}

fn wrap_ranges(body: &str, width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    loop {
        let mut end = start;
        let mut columns = 0;
        let mut word_break = None;
        for (index, grapheme) in body[start..].grapheme_indices(true) {
            let next = grapheme.width();
            if columns + next > width.max(1) && end > start {
                end = word_break.unwrap_or(end);
                break;
            }
            columns += next;
            end = start + index + grapheme.len();
            if grapheme.chars().all(char::is_whitespace) {
                word_break = Some(end);
            }
        }
        rows.push(start..end);
        if end == body.len() {
            return rows;
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_wrap_and_scrolling_keeps_the_paused_record_and_count() {
        let mut log = Log::default();
        let viewport = LogViewport {
            height: 2,
            width: 8,
        };
        log.receive(b"AAAAAAA BBBBBBB CCCCCCC DDDDDDD\n");
        log.toggle_follow();
        log.receive(b"new output\n");
        log.horizontal = 8;

        log.toggle_wrap(Mode::Rx, Language::En, Format::Text, viewport);
        let records = log.wrapped(Mode::Rx, Language::En, Format::Text, viewport);
        assert_eq!(&records[0].body[records[0].rows[0].clone()], "BBBBBBB ");
        assert_eq!(&records[0].body[records[0].rows[1].clone()], "CCCCCCC ");

        log.scroll_wrapped(Mode::Rx, Language::En, Format::Text, viewport, 1);
        let records = log.wrapped(Mode::Rx, Language::En, Format::Text, viewport);
        assert_eq!(&records[0].body[records[0].rows[1].clone()], "DDDDDDD");
        log.toggle_wrap(Mode::Rx, Language::En, Format::Text, viewport);
        assert_eq!(log.horizontal, 16);
        assert_eq!(log.visible(Mode::Rx, 1)[0].id, 1);
        assert_eq!(log.new_since_pause(), Some(1));
    }
}
