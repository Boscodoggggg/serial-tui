use std::{cmp::Ordering, ops::Range};

use unicode_width::UnicodeWidthStr;

use super::{
    Direction, Editor, Format, Language, Log, LogEntry, LogViewport, Mode, decode_hex,
    log_view::wrapped_anchor,
};

pub struct SearchInput {
    pub editor: Editor,
    pub hex: bool,
    pub rx_only: bool,
}

impl SearchInput {
    pub(super) fn new(previous: Option<&LogSearch>, display: Format) -> Self {
        Self {
            editor: previous
                .map_or_else(Editor::default, |search| Editor::new(search.input.clone())),
            hex: previous.map_or(display != Format::Text, |search| {
                search.format() == Format::Hex
            }),
            rx_only: previous.is_some_and(|search| search.rx_only),
        }
    }

    pub(super) fn compile(&self) -> Result<LogSearch, &'static str> {
        let input = &self.editor.value;
        if input.is_empty() {
            return Err("search_empty");
        }
        let pattern = if self.hex {
            Pattern::Hex(decode_hex(input)?)
        } else {
            Pattern::Text
        };
        Ok(LogSearch {
            input: input.clone(),
            pattern,
            rx_only: self.rx_only,
            current: None,
        })
    }
}

enum Pattern {
    Text,
    Hex(Vec<u8>),
}

struct Hit {
    id: u64,
    range: Range<usize>,
}

pub(super) struct LogSearch {
    pub input: String,
    pub rx_only: bool,
    pattern: Pattern,
    current: Option<Hit>,
}

impl LogSearch {
    pub(super) fn format(&self) -> Format {
        match self.pattern {
            Pattern::Text => Format::Text,
            Pattern::Hex(_) => Format::Hex,
        }
    }

    pub(super) fn clear_hit(&mut self) {
        self.current = None;
    }

    pub(super) fn highlight(&self, entry: &LogEntry, display: Format) -> Option<Range<usize>> {
        let hit = self.current.as_ref().filter(|hit| hit.id == entry.id)?;
        match self.pattern {
            Pattern::Text if display == Format::Text || entry.direction == Direction::System => {
                Some(hit.range.clone())
            }
            Pattern::Hex(_) if display != Format::Text => {
                Some(hit.range.start * 3..hit.range.end * 3 - 1)
            }
            _ => None,
        }
    }

    pub(super) fn jump(
        &mut self,
        log: &mut Log,
        mode: Mode,
        lang: Language,
        viewport: LogViewport,
        reverse: bool,
    ) -> bool {
        let entries = log.filtered(mode);
        let mut order = (0..entries.len())
            .map(|index| {
                if reverse {
                    entries.len() - 1 - index
                } else {
                    index
                }
            })
            .filter(|&index| !self.rx_only || entries[index].direction == Direction::Rx);
        let find = |index: usize, after: Option<&Hit>| {
            let entry = entries[index];
            let boundary = match after {
                Some(hit) => match entry.id.cmp(&hit.id) {
                    Ordering::Equal => Some(hit.range.start),
                    Ordering::Less if !reverse => return None,
                    Ordering::Greater if reverse => return None,
                    _ => None,
                },
                None => None,
            };
            self.find_in(entry, lang, reverse, boundary)
                .map(|range| (index, range))
        };
        let next = self
            .current
            .as_ref()
            .and_then(|current| order.clone().find_map(|index| find(index, Some(current))));
        let Some((index, range)) = next.or_else(|| order.find_map(|index| find(index, None)))
        else {
            self.clear_hit();
            return false;
        };
        let entry = entries[index];
        let column = match self.pattern {
            Pattern::Text => entry.body(lang, Format::Text)[..range.start].width(),
            Pattern::Hex(_) => range.start * 3,
        };
        let (anchor, byte) = if log.wrap {
            let start = match self.pattern {
                Pattern::Text => range.start,
                Pattern::Hex(_) => range.start * 3,
            };
            let body = entry.body(lang, self.format());
            (entry.id, Some(wrapped_anchor(&body, viewport, start)))
        } else {
            let bottom = (index + viewport.height.saturating_sub(1) / 2)
                .max(viewport.height.min(entries.len()).saturating_sub(1))
                .min(entries.len() - 1);
            (entries[bottom].id, None)
        };
        self.current = Some(Hit {
            id: entry.id,
            range,
        });
        log.horizontal = column.saturating_sub(8);
        log.pause_at_byte(anchor, byte);
        true
    }

    fn find_in(
        &self,
        entry: &LogEntry,
        lang: Language,
        reverse: bool,
        boundary: Option<usize>,
    ) -> Option<Range<usize>> {
        let text;
        let (data, needle) = match &self.pattern {
            Pattern::Text => {
                text = entry.body(lang, Format::Text);
                (text.as_bytes(), self.input.as_bytes())
            }
            Pattern::Hex(bytes) => (entry.data.as_slice(), bytes.as_slice()),
        };
        let mut hits = data
            .windows(needle.len())
            .enumerate()
            .filter(|(start, slice)| {
                *slice == needle
                    && boundary.is_none_or(|boundary| {
                        if reverse {
                            *start < boundary
                        } else {
                            *start > boundary
                        }
                    })
            });
        let (start, _) = if reverse {
            hits.next_back()
        } else {
            hits.next()
        }?;
        Some(start..start + needle.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_search_wraps_between_matches_in_the_same_rx_record_and_skips_tx() {
        let mut log = Log::default();
        let mut rx = vec![b'x'; 64];
        rx.extend_from_slice(b"\x01\x03\xff\x01\x03\n");
        log.receive(&rx);
        log.sent(vec![0x01, 0x03]);
        let mut search = SearchInput {
            editor: Editor::new("01 03".into()),
            hex: true,
            rx_only: true,
        }
        .compile()
        .unwrap();

        let viewport = LogViewport {
            height: 2,
            width: 16,
        };
        assert!(search.jump(&mut log, Mode::Both, Language::En, viewport, true));
        assert!(log.is_paused());
        let entries = log.visible(Mode::Both, 2);
        assert_eq!(search.highlight(entries[0], Format::Hex), Some(201..206));
        assert_eq!(search.highlight(entries[1], Format::Hex), None);
        assert!(log.horizontal > 0);

        assert!(search.jump(&mut log, Mode::Both, Language::En, viewport, false));
        let entries = log.visible(Mode::Both, 2);
        assert_eq!(search.highlight(entries[0], Format::Hex), Some(192..197));
        assert!(search.jump(&mut log, Mode::Both, Language::En, viewport, true));
        let entries = log.visible(Mode::Both, 2);
        assert_eq!(search.highlight(entries[0], Format::Hex), Some(201..206));
    }
}
