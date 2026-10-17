//! Sans-I/O terminal state owner. `screen_replay` is a limited visible-cell probe;
//! `restore_sequence` is the evaluated complete-state restore (see
//! docs/architecture/terminal-restore-evaluation.md for coverage and gaps).
#![forbid(unsafe_code)]

mod cold_history;
mod ground;
/// Bounded recent-history VT replay for attachment transactions.
pub mod history;
mod history_search;
pub use history_search::{HistorySearch, HistorySearchRequest};
mod history_page;
pub use history_page::{HistoryPage, HistoryPageError, HistoryRow};
#[cfg(test)]
mod history_page_tests;
/// Bounded, untrusted OSC 133 hints; never session authorization or command execution.
pub mod shell_markers;
pub use history::{HistoryReplay, MAX_RECENT_ROWS};
mod palette;
mod primary_text;
pub mod reconciliation;
mod restore;
mod tracked;
pub use restore::{Restore, RestoreGap};
/// Window-independent actor and bounded test transport.
pub mod session;
pub use palette::QueryPalette;

/// Actual Unicode table version shared with the authoritative parser.
pub const UNICODE_VERSION: (u8, u8, u8) = unicode_width::UNICODE_VERSION;

/// Non-CJK scalar width used by the parser (non-printing controls map to zero).
pub fn scalar_width(codepoint: u32) -> u8 {
    char::from_u32(codepoint)
        .and_then(unicode_width::UnicodeWidthChar::width)
        .unwrap_or(0) as u8
}

#[cfg(any(test, feature = "validation"))]
use std::fmt::Write;
use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Osc52, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use pulse_core::TermSize;
use serde::Serialize;

struct Size(TermSize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows())
    }
    fn columns(&self) -> usize {
        usize::from(self.0.columns())
    }
}

#[derive(Clone)]
struct ReplyListener(Sender<Event>);
impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        if matches!(
            event,
            Event::PtyWrite(_) | Event::ColorRequest(..) | Event::TextAreaSizeRequest(_)
        ) {
            let _ = self.0.send(event);
        }
        // Clipboard/title requests remain disabled; no GUI callbacks are invoked.
    }
}

/// A normalized visible cell for comparison across different terminal parsers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedCell {
    /// Unicode content, including combining characters.
    pub text: String,
    /// Physical cell width: 0 for a wide-character spacer.
    pub width: u8,
    /// default, idx:N or rgb:R,G,B.
    pub foreground: String,
    /// default, idx:N or rgb:R,G,B.
    pub background: String,
    /// Bold style.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Single underline (the initial corpus does not cover other underline styles).
    pub underline: bool,
    /// Inverted foreground/background.
    pub inverse: bool,
}

/// Visible-screen projection, not a complete serialized terminal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedScreen {
    /// Rows in viewport order.
    pub cells: Vec<Vec<ObservedCell>>,
    /// Zero-based physical cursor coordinates.
    pub cursor: (usize, usize),
    /// Alternate screen active.
    pub alternate: bool,
}

/// Owns the parser, grid and query replies without any window or transport.
pub struct TerminalProbe {
    term: Term<ReplyListener>,
    cold: cold_history::ColdHistory,
    parser: Processor,
    events: Receiver<Event>,
    replies: Vec<String>,
    palette: QueryPalette,
    cell_pixels: Option<(u16, u16)>,
    utf8_tail: Vec<u8>,
    side: tracked::MirroredState,
    ground: ground::GroundScanner,
    shell_markers: shell_markers::MarkerQueue,
}

impl TerminalProbe {
    /// Construct a bounded terminal with caller-selected history length.
    pub fn new(size: TermSize, history: usize) -> Self {
        let (sender, replies) = mpsc::channel();
        let config = Config {
            scrolling_history: history.min(10_000),
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        Self {
            side: tracked::MirroredState::new(
                usize::from(size.columns()),
                usize::from(size.rows()),
            ),
            ground: ground::GroundScanner::default(),
            shell_markers: shell_markers::MarkerQueue::default(),
            term: Term::new(config, &Size(size), ReplyListener(sender)),
            cold: cold_history::ColdHistory::new(history.min(10_000)),
            parser: Processor::new(),
            events: replies,
            replies: Vec::new(),
            palette: QueryPalette::default(),
            cell_pixels: None,
            utf8_tail: Vec::new(),
        }
    }

    /// Consume every byte, preserving parser state across arbitrary chunk boundaries.
    pub fn feed(&mut self, bytes: &[u8]) {
        // vte 0.15 may consume the following ASCII character when completing a
        // split codepoint. Keep only incomplete trailing UTF-8 here, so each
        // advance starts and ends on codepoint boundaries. Invalid bytes are
        // preserved (including C1 controls); this is not lossy transcoding.
        if self.utf8_tail.is_empty() {
            self.feed_complete_prefix(bytes);
        } else {
            let mut joined = std::mem::take(&mut self.utf8_tail);
            joined.extend_from_slice(bytes);
            self.feed_complete_prefix(&joined);
        }
    }

    /// End an input stream, replacing one trailing incomplete UTF-8 scalar with U+FFFD.
    /// Repeated calls are harmless. Flushes any unclosed synchronized update;
    /// does not reset or serialize the remaining ANSI parser state.
    pub fn finish_input(&mut self) {
        if !self.utf8_tail.is_empty() {
            self.utf8_tail.clear();
            self.advance("\u{fffd}".as_bytes());
        }
        self.flush_sync();
        self.collect_events();
    }

    /// Product actors retain bounded compact history beyond the hot restore grid.
    /// Build retained history with an explicit hot+cold row limit.
    pub fn with_history_limit(size: TermSize, hot: usize, limit: usize) -> Self {
        let mut terminal = Self::with_cold_history(size, hot.min(limit));
        terminal.cold.set_limit(limit);
        terminal
    }
    pub(crate) fn with_cold_history(size: TermSize, hot_rows: usize) -> Self {
        let mut terminal = Self::new(size, hot_rows);
        terminal.cold.enable(&mut terminal.term);
        terminal
    }
    /// Deadline supplied by VTE for the current synchronized update, if any.
    pub fn sync_deadline(&self) -> Option<std::time::Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    /// Apply a timed-out synchronized update through the same tracked handler.
    /// The caller schedules this even when no new output arrives. Not EOF.
    pub fn expire_sync(&mut self, now: std::time::Instant) -> bool {
        if self.sync_deadline().is_some_and(|deadline| now >= deadline) {
            self.flush_sync();
            self.collect_events();
            true
        } else {
            false
        }
    }

    fn flush_sync(&mut self) {
        if self.sync_deadline().is_some() || self.parser.sync_bytes_count() > 0 {
            let mut handler = tracked::Tracked {
                term: &mut self.term,
                side: &mut self.side,
                history: &mut self.cold,
            };
            self.parser.stop_sync(&mut handler);
        }
    }

    /// Resize the owned terminal state using validated character dimensions.
    pub fn resize(&mut self, size: TermSize) {
        let old = (self.term.columns(), self.term.screen_lines());
        self.term.resize(Size(size));
        self.cold.collect(&mut self.term);
        self.side
            .on_resize(old, (self.term.columns(), self.term.screen_lines()));
    }

    /// Drain authoritative protocol replies; a transport would write these upstream.
    pub fn take_replies(&mut self) -> Vec<String> {
        std::mem::take(&mut self.replies)
    }

    /// Drain untrusted shell hints with an explicit count of discarded excess markers.
    /// No coordinates, complete command log, snapshot state, or capabilities are implied.
    pub fn take_shell_markers(&mut self) -> shell_markers::ShellMarkerBatch {
        self.shell_markers.take()
    }

    /// Update the fallback palette from the current UI theme; OSC overrides win
    /// until reset by the program. Already queued replies are not recolored.
    pub fn set_query_palette(&mut self, palette: QueryPalette) {
        self.palette = palette;
    }

    /// Base rendering/query colors, never remote OSC overrides or application authority.
    pub fn query_palette(&self) -> &QueryPalette {
        &self.palette
    }

    /// Actual measured cell dimensions, or None when pixel geometry is unknown.
    /// Overflowing/zero geometry is never passed to backend formatters.
    pub fn set_cell_pixels(&mut self, pixels: Option<(u16, u16)>) {
        self.cell_pixels = pixels;
    }

    /// Inspect the active viewport. Scrollback and hidden state are intentionally excluded.
    pub fn screen(&self) -> ObservedScreen {
        let grid = self.term.grid();
        let cells = (0..grid.screen_lines())
            .map(|row| {
                (0..grid.columns())
                    .map(|col| observe(&grid[Point::new(Line(row as i32), Column(col))]))
                    .collect()
            })
            .collect();
        let cursor = grid.cursor.point;
        ObservedScreen {
            cells,
            cursor: (cursor.column.0, cursor.line.0 as usize),
            alternate: self.term.mode().contains(TermMode::ALT_SCREEN),
        }
    }

    /// Produce a VT replay of the visible cells for feasibility tests only.
    /// Do not use this for detach/attach until the hidden-state requirements are met.
    #[cfg(any(test, feature = "validation"))]
    pub fn screen_replay(&self) -> String {
        let mut output = String::from("\x1bc");
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            output.push_str("\x1b[?1049h");
        }
        let grid = self.term.grid();
        for row in 0..grid.screen_lines() {
            for col in 0..grid.columns() {
                let cell = &grid[Point::new(Line(row as i32), Column(col))];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let _ = write!(output, "\x1b[{};{}H\x1b[0m", row + 1, col + 1);
                sgr_color(&mut output, cell.fg, true);
                sgr_color(&mut output, cell.bg, false);
                for (flag, code) in [
                    (Flags::BOLD, 1),
                    (Flags::ITALIC, 3),
                    (Flags::UNDERLINE, 4),
                    (Flags::INVERSE, 7),
                ] {
                    if cell.flags.contains(flag) {
                        let _ = write!(output, "\x1b[{code}m");
                    }
                }
                // A literal tab stored by the parser is a blank physical cell, not an HT action.
                output.push(if cell.c == '\t' { ' ' } else { cell.c });
                if let Some(combining) = cell.zerowidth() {
                    output.extend(combining);
                }
            }
        }
        let cursor = grid.cursor.point;
        let _ = write!(
            output,
            "\x1b[0m\x1b[{};{}H",
            cursor.line.0 + 1,
            cursor.column.0 + 1
        );
        for (mode, code) in [
            (TermMode::APP_CURSOR, 1),
            (TermMode::SHOW_CURSOR, 25),
            (TermMode::LINE_WRAP, 7),
            (TermMode::BRACKETED_PASTE, 2004),
        ] {
            let end = if self.term.mode().contains(mode) {
                'h'
            } else {
                'l'
            };
            let _ = write!(output, "\x1b[?{code}{end}");
        }
        output
    }

    /// Actual retained history line count.
    pub fn history_lines(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Lower-bound cell storage only; excludes allocation overhead and extras.
    pub fn grid_cell_bytes(&self) -> usize {
        self.term.grid().total_lines() * self.term.grid().columns() * size_of::<Cell>()
    }
}

// Find only a trailing incomplete codepoint, skipping malformed bytes without
// altering them. At most three bytes need to wait for the next transport read.
fn complete_utf8_prefix(bytes: &[u8]) -> usize {
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(_) => return bytes.len(),
            Err(error) => {
                offset += error.valid_up_to();
                match error.error_len() {
                    Some(length) => offset += length,
                    None => return offset,
                }
            }
        }
    }
    bytes.len()
}

impl TerminalProbe {
    fn collect_events(&mut self) {
        for event in self.events.try_iter() {
            match event {
                Event::PtyWrite(reply) => self.replies.push(reply),
                Event::ColorRequest(index, format)
                    if index < alacritty_terminal::term::color::COUNT =>
                {
                    let override_color = self
                        .side
                        .color_queries
                        .pop_front()
                        .unwrap_or(self.term.colors()[index]);
                    let color = override_color.or_else(|| self.palette.indexed(index));
                    if let Some(color) = color {
                        self.replies.push(format(color));
                    }
                }
                Event::TextAreaSizeRequest(format) => {
                    if let Some((cell_width, cell_height)) = self.cell_pixels {
                        let num_lines = self.term.screen_lines() as u16;
                        let num_cols = self.term.columns() as u16;
                        if cell_width > 0
                            && cell_height > 0
                            && num_lines.checked_mul(cell_height).is_some()
                            && num_cols.checked_mul(cell_width).is_some()
                        {
                            self.replies.push(format(WindowSize {
                                num_lines,
                                num_cols,
                                cell_width,
                                cell_height,
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    /// Every parsed byte goes through the transparent mirror and the boundary scanner.
    fn advance(&mut self, bytes: &[u8]) {
        let mut handler = tracked::Tracked {
            term: &mut self.term,
            side: &mut self.side,
            history: &mut self.cold,
        };
        self.parser.advance(&mut handler, bytes);
        let markers = &mut self.shell_markers;
        self.ground
            .feed_with_osc(bytes, |byte, body| markers.observe(byte, body));
    }

    fn feed_complete_prefix(&mut self, bytes: &[u8]) {
        let end = complete_utf8_prefix(bytes);
        // Answer at OSC terminators before a later OSC set/reset or RIS can
        // change the queried color. These delimiters cannot split valid UTF-8.
        // Plain output remains batched; this is not byte-at-a-time parsing.
        let mut start = 0;
        for (index, byte) in bytes[..end].iter().enumerate() {
            if matches!(byte, 0x07 | b'\\' | 0x9c) {
                self.advance(&bytes[start..=index]);
                self.collect_events();
                start = index + 1;
            }
        }
        self.advance(&bytes[start..end]);
        self.collect_events();
        self.utf8_tail.extend_from_slice(&bytes[end..]);
        debug_assert!(self.utf8_tail.len() <= 3);
    }
}

fn observe(cell: &Cell) -> ObservedCell {
    let spacer = cell.flags.contains(Flags::WIDE_CHAR_SPACER);
    let mut text = if spacer {
        String::new()
    } else if cell.c == '\t' {
        String::from(" ")
    } else {
        cell.c.to_string()
    };
    if let Some(combining) = cell.zerowidth() {
        text.extend(combining);
    }
    ObservedCell {
        text,
        width: if spacer {
            0
        } else if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        },
        foreground: color_label(cell.fg),
        background: color_label(cell.bg),
        bold: cell.flags.contains(Flags::BOLD),
        italic: cell.flags.contains(Flags::ITALIC),
        underline: cell.flags.contains(Flags::UNDERLINE),
        inverse: cell.flags.contains(Flags::INVERSE),
    }
}

fn color_label(color: Color) -> String {
    match color {
        Color::Named(NamedColor::Foreground | NamedColor::Background) => String::from("default"),
        Color::Named(color) => format!("idx:{}", color as usize),
        Color::Indexed(index) => format!("idx:{index}"),
        Color::Spec(rgb) => format!("rgb:{},{},{}", rgb.r, rgb.g, rgb.b),
    }
}

fn sgr_color(output: &mut impl std::fmt::Write, color: Color, foreground: bool) {
    let channel = if foreground { 38 } else { 48 };
    match color {
        Color::Named(NamedColor::Foreground | NamedColor::Background) => {}
        // Keep the palette class: SGR 30-37/90-97 (named) and 38;5;n (indexed)
        // differ for bold-as-bright rendering in both alacritty and xterm.js.
        Color::Named(color) if (color as usize) < 8 => {
            let base = if foreground { 30 } else { 40 };
            let _ = write!(output, "\x1b[{}m", base + color as usize);
        }
        Color::Named(color) if (color as usize) < 16 => {
            let base = if foreground { 90 } else { 100 };
            let _ = write!(output, "\x1b[{}m", base + color as usize - 8);
        }
        Color::Named(color) => {
            let _ = write!(output, "\x1b[{channel};5;{}m", color as usize);
        }
        Color::Indexed(index) => {
            let _ = write!(output, "\x1b[{channel};5;{index}m");
        }
        Color::Spec(rgb) => {
            let _ = write!(output, "\x1b[{channel};2;{};{};{}m", rgb.r, rgb.g, rgb.b);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn terminal() -> TerminalProbe {
        TerminalProbe::new(TermSize::new(24, 6).unwrap(), 64)
    }

    #[test]
    fn queued_queries_do_not_use_a_later_theme() {
        let mut term = terminal();
        term.feed(b"\x1b]10;?\x07");
        let palette = QueryPalette {
            foreground: [0x12, 0x34, 0x56],
            ..QueryPalette::default()
        };
        term.set_query_palette(palette);
        term.feed(b"\x1b]10;?\x07");
        assert_eq!(
            term.take_replies(),
            vec![
                "\x1b]10;rgb:ffff/ffff/ffff\x07",
                "\x1b]10;rgb:1212/3434/5656\x07"
            ]
        );
    }
    #[test]
    fn remote_override_survives_theme_update_until_reset() {
        let mut term = terminal();
        term.feed(b"\x1b]10;#abcdef\x07");
        term.set_query_palette(QueryPalette {
            foreground: [1, 2, 3],
            ..QueryPalette::default()
        });
        term.feed(b"\x1b]10;?\x07\x1b]110\x07\x1b]10;?\x07");
        assert_eq!(
            term.take_replies(),
            vec![
                "\x1b]10;rgb:abab/cdcd/efef\x07",
                "\x1b]10;rgb:0101/0202/0303\x07"
            ]
        );
    }
    #[test]
    fn base_palette_queries_follow_osc_overrides_ris_and_explicit_resets() {
        let mut term = terminal();
        let query = b"\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07";
        let base = QueryPalette {
            ansi: std::array::from_fn(|i| [i as u8, 0x11, 0x22]),
            foreground: [0x11, 0x22, 0x33],
            background: [0x44, 0x55, 0x66],
            cursor: [0x77, 0x88, 0x99],
        };
        term.set_query_palette(base.clone());
        term.feed(query);
        let expected_base = [
            "\x1b]4;1;rgb:0101/1111/2222\x07",
            "\x1b]10;rgb:1111/2222/3333\x07",
            "\x1b]11;rgb:4444/5555/6666\x07",
            "\x1b]12;rgb:7777/8888/9999\x07",
        ];
        assert_eq!(term.take_replies(), expected_base);
        term.feed(b"\x1b]4;1;#abcdef\x07\x1b]10;#102030\x07\x1b]11;#203040\x07\x1b]12;#304050\x07");
        let overrides = [
            "\x1b]4;1;rgb:abab/cdcd/efef\x07",
            "\x1b]10;rgb:1010/2020/3030\x07",
            "\x1b]11;rgb:2020/3030/4040\x07",
            "\x1b]12;rgb:3030/4040/5050\x07",
        ];
        term.feed(query);
        assert_eq!(term.take_replies(), overrides);
        let changed = QueryPalette {
            ansi: std::array::from_fn(|i| [i as u8, 0x23, 0x45]),
            foreground: [0x12, 0x34, 0x56],
            background: [0x78, 0x9a, 0xbc],
            cursor: [0xde, 0xf0, 0x13],
        };
        term.set_query_palette(changed);
        term.feed(b"\x1bc");
        term.feed(query);
        assert_eq!(term.take_replies(), overrides);
        term.feed(b"\x1b]104\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07");
        term.feed(query);
        assert_eq!(
            term.take_replies(),
            [
                "\x1b]4;1;rgb:0101/2323/4545\x07",
                "\x1b]10;rgb:1212/3434/5656\x07",
                "\x1b]11;rgb:7878/9a9a/bcbc\x07",
                "\x1b]12;rgb:dede/f0f0/1313\x07",
            ]
        );
    }

    #[test]
    fn pixel_geometry_uses_current_dimensions_and_never_overflows() {
        let mut term = terminal();
        term.feed(b"\x1b[14t");
        assert!(term.take_replies().is_empty());
        term.set_cell_pixels(Some((8, 16)));
        term.feed(b"\x1b[14t");
        assert_eq!(term.take_replies(), vec!["\x1b[4;96;192t"]);
        term.resize(TermSize::new(32, 8).unwrap());
        term.feed(b"\x1b[14t");
        assert_eq!(term.take_replies(), vec!["\x1b[4;128;256t"]);
        for dimensions in [(0, 16), (8, 0), (u16::MAX, 16), (8, u16::MAX)] {
            term.set_cell_pixels(Some(dimensions));
            term.feed(b"\x1b[14t");
            assert!(term.take_replies().is_empty());
        }
    }
    #[test]
    fn indexed_palette_covers_cube_and_grayscale_boundaries() {
        let palette = QueryPalette::default();
        for (index, expected) in [
            (16, [0, 0, 0]),
            (231, [255, 255, 255]),
            (232, [8, 8, 8]),
            (255, [238, 238, 238]),
        ] {
            let rgb = palette.indexed(index).unwrap();
            assert_eq!([rgb.r, rgb.g, rgb.b], expected);
        }
        assert!(palette.indexed(269).is_none());
    }
    #[test]
    fn arbitrary_chunk_boundaries_preserve_state() {
        let input = "hello\r\n\x1b[31m中文 e\u{301} 🌍\x1b[0m\x1b[2;4H!";
        let mut whole = terminal();
        whole.feed(input.as_bytes());
        for chunk_size in 1..=input.len() {
            let mut chunked = terminal();
            for chunk in input.as_bytes().chunks(chunk_size) {
                chunked.feed(chunk);
            }
            let expected = whole.screen();
            let actual = chunked.screen();
            for (row, (expected_row, actual_row)) in
                expected.cells.iter().zip(&actual.cells).enumerate()
            {
                for (col, (expected_cell, actual_cell)) in
                    expected_row.iter().zip(actual_row).enumerate()
                {
                    assert_eq!(
                        expected_cell, actual_cell,
                        "chunk size {chunk_size}, cell {row},{col}"
                    );
                }
            }
            assert_eq!(expected.cursor, actual.cursor, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn split_combining_character_keeps_following_space() {
        let mut source = terminal();
        source.feed(b"e\xcc");
        source.feed(b"\x81 \xf0\x9f\x8c\x8d");
        assert_eq!(source.screen().cells[0][0].text, "e\u{301}");
        assert_eq!(source.screen().cells[0][1].text, " ");
        assert_eq!(source.screen().cells[0][2].text, "🌍");
    }

    #[test]
    fn utf8_boundary_keeps_invalid_bytes_and_controls() {
        assert_eq!(complete_utf8_prefix(b"\xff\x1b[31m\xe4\xb8"), 6);
        assert_eq!(complete_utf8_prefix(b"\x9b\xff"), 2);
        let mut source = terminal();
        source.feed(b"\xe4");
        assert_eq!(source.utf8_tail.len(), 1);
        source.feed(b"\xb8");
        assert_eq!(source.utf8_tail.len(), 2);
        source.feed(b"\xad!");
        assert!(source.utf8_tail.is_empty());
        assert_eq!(source.screen().cells[0][0].text, "中");
        assert_eq!(source.screen().cells[0][2].text, "!");
    }

    #[test]
    fn visible_replay_matches_cells_and_cursor() {
        let mut source = terminal();
        source.feed("\x1b[1;3;4;7;38;2;12;34;56m中e\u{301}\x1b[0m\r\nready> ".as_bytes());
        let mut restored = terminal();
        restored.feed(source.screen_replay().as_bytes());
        assert_eq!(source.screen(), restored.screen());
    }

    #[test]
    fn query_has_one_backend_reply_and_clipboard_is_disabled() {
        let mut source = terminal();
        source.feed(b"abc\x1b[6n\x1b]52;c;?\x07");
        assert_eq!(source.take_replies(), vec!["\x1b[1;4R"]);
        assert!(source.take_replies().is_empty());
    }

    #[test]
    fn eof_replaces_each_incomplete_scalar_once() {
        for bytes in [
            vec![0xc2],
            vec![0xe4],
            vec![0xe4, 0xb8],
            vec![0xf0],
            vec![0xf0, 0x9f],
            vec![0xf0, 0x9f, 0x8c],
        ] {
            let mut source = terminal();
            source.feed(b"ready:");
            source.feed(&bytes);
            assert_eq!(source.screen().cursor, (6, 0));
            source.finish_input();
            let screen = source.screen();
            assert_eq!(screen.cells[0][6].text, "\u{fffd}");
            assert_eq!(screen.cursor, (7, 0));
            assert!(source.utf8_tail.is_empty());
            source.finish_input();
            assert_eq!(source.screen(), screen);
        }
    }

    #[test]
    fn complete_input_and_partial_queries_are_unchanged_at_eof() {
        for input in ["中🌍", "abc\x1b[", "abc\x1b]52;c;?"] {
            let mut source = terminal();
            source.feed(input.as_bytes());
            let screen = source.screen();
            source.finish_input();
            assert_eq!(source.screen(), screen);
            assert!(source.take_replies().is_empty());
        }
    }

    proptest::proptest! {
        #[test]
        fn generated_streams_preserve_state_and_replies(
            tokens in proptest::collection::vec(0usize..12, 0..64),
            partition in proptest::collection::vec(1usize..17, 1..12),
        ) {
            let alphabet = ["text ", "中", "🌍", "e\u{301}", "\r\n", "\x1b[31m", "\x1b[0m", "\x1b[6n", "\x1b[?25$p", "\x1b[?1049h", "\x1b[?1049l", "\x1b[2;4H"];
            let input: String = tokens.into_iter().map(|token| alphabet[token]).collect();
            let mut whole = terminal();
            whole.feed(input.as_bytes());
            whole.finish_input();
            let mut chunked = terminal();
            let mut offset = 0;
            let mut step = 0;
            let mut replies = Vec::new();
            while offset < input.len() {
                let end = (offset + partition[step % partition.len()]).min(input.len());
                chunked.feed(&input.as_bytes()[offset..end]);
                replies.extend(chunked.take_replies());
                offset = end;
                step += 1;
            }
            chunked.finish_input();
            replies.extend(chunked.take_replies());
            proptest::prop_assert_eq!(chunked.screen(), whole.screen());
            proptest::prop_assert_eq!(replies, whole.take_replies());
        }
    }

    #[test]
    fn sync_deadline_is_observed_and_expired_once() {
        let mut term = terminal();
        term.feed(b"\x1b[?2026habc\x1b[6n");
        let deadline = term.sync_deadline().unwrap();
        assert!(!term.expire_sync(deadline - std::time::Duration::from_millis(1)));
        assert!(matches!(
            term.restore_sequence(),
            Err(RestoreGap::SyncPending)
        ));
        assert!(term.expire_sync(deadline));
        assert!(!term.expire_sync(deadline));
        assert_eq!(term.take_replies(), ["\x1b[1;4R"]);
        assert!(term.restore_sequence().is_ok());
    }
    #[test]
    fn synchronized_color_queries_keep_query_time_overrides_across_ris() {
        for ending in [b"\x1b[?2026l".as_slice(), b"".as_slice()] {
            let mut term = terminal();
            term.feed(
                b"\x1b[?2026h\x1b]10;?\x07\x1b]10;#123456\x07\x1b]10;?\x07\x1bc\x1b]10;?\x07\x1b]110\x07\x1b]10;?\x07",
            );
            term.feed(ending);
            term.finish_input();
            assert_eq!(
                term.take_replies(),
                [
                    "\x1b]10;rgb:ffff/ffff/ffff\x07",
                    "\x1b]10;rgb:1212/3434/5656\x07",
                    "\x1b]10;rgb:1212/3434/5656\x07", // RIS retains the upstream color override.
                    "\x1b]10;rgb:ffff/ffff/ffff\x07"
                ]
            );
            assert!(term.side.color_queries.is_empty());
        }
    }
    #[test]
    fn history_is_bounded_without_an_attached_view() {
        let mut source = terminal();
        for _ in 0..100_000 {
            source.feed(b"sample log line\r\n");
        }
        assert_eq!(source.history_lines(), 64);
        assert!(source.grid_cell_bytes() > 0);
    }

    // ---- Complete-restore evaluation spike ----

    const RESTORE_TOKENS: &[&str] = &[
        "a",
        "Z",
        "q",
        "中",
        "é",
        "e\u{301}",
        "\r",
        "\n",
        "\t",
        "\x08",
        "123456789012345678901234",
        "\x1b[2;5r",
        "\x1b[r",
        "\x1b[?6h",
        "\x1b[?6l",
        "\x1b[3;7H",
        "\x1b[H",
        "\x1b[6;22H",
        "\x1b7",
        "\x1b8",
        "\x1b[s",
        "\x1b[u",
        "\x1b(0",
        "\x1b(B",
        "\x1b)0",
        "\x0e",
        "\x0f",
        "\x1b[4h",
        "\x1b[4l",
        "\x1b[20h",
        "\x1b[20l",
        "\x1b[?7l",
        "\x1b[?7h",
        "\x1bH",
        "\x1b[g",
        "\x1b[3g",
        "\x1b[5G",
        "\x1b[1;31m",
        "\x1b[4:3m",
        "\x1b[58;5;3m",
        "\x1b[0m",
        "\x1b[38;2;1;2;3;48;5;200m",
        "\x1b[2;8;9m",
        "\x1b]4;1;#123456\x07",
        "\x1b]10;#abcdef\x07",
        "\x1b]104\x07",
        "\x1b[?1049h",
        "\x1b[?1049l",
        "\x1b[?3h",
        "\x1b[?3l",
        "\x1b[5 q",
        "\x1b[?12h",
        "\x1b[?25l",
        "\x1b[?2004h",
        "\x1b[?1002h",
        "\x1b[?1006h",
        "\x1b[?1h",
        "\x1b=",
        "\x1b>",
        "\x1b]8;;http://x\x1b\\",
        "\x1b]8;;\x1b\\",
        "\x1b[2J",
        "\x1b[K",
        "\x1b[L",
        "\x1b[M",
        "\x1b[P",
        "\x1b[@",
        "\x1bM",
        "\x1bD",
        "\x1bE",
        "\x1b[3S",
        "\x1b[2T",
        "\x1bc",
        "\x1b[3;",
        "\x1b]2;ti",
        "\x1b[?2026h",
        "\x1b[?2026l",
        "\x1b[3b",
        "\x1b[3X",
        "\x1b[2Z",
        "\x1b[?47h",
    ];

    /// Concise first difference between two fingerprints, for shrunk failures.
    fn fingerprint_diff(expected: &str, actual: &str) -> Option<String> {
        expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(index, (a, b))| {
                let cells_a: Vec<&str> = a.split('|').collect();
                let cells_b: Vec<&str> = b.split('|').collect();
                match cells_a.iter().zip(&cells_b).position(|(x, y)| x != y) {
                    Some(col) if cells_a.len() > 2 => format!(
                        "line {index} col {col}: expected {} actual {}",
                        cells_a[col], cells_b[col]
                    ),
                    _ => format!("line {index}: expected {a} actual {b}"),
                }
            })
    }

    fn token_names(tokens: &[usize]) -> Vec<String> {
        tokens
            .iter()
            .map(|&i| format!("{:?}", RESTORE_TOKENS[i]))
            .collect()
    }

    fn tokens_to_bytes(tokens: &[usize]) -> Vec<u8> {
        tokens
            .iter()
            .flat_map(|&i| RESTORE_TOKENS[i].bytes())
            .collect()
    }

    #[test]
    fn tracked_handler_is_transparent() {
        use alacritty_terminal::event::VoidListener;
        let size = TermSize::new(24, 6).unwrap();
        let mut runner = proptest::test_runner::TestRunner::default();
        runner
            .run(
                &proptest::collection::vec(0..RESTORE_TOKENS.len(), 0..80),
                |tokens| {
                    let bytes = tokens_to_bytes(&tokens);
                    let mut probe = TerminalProbe::new(size, 64);
                    probe.feed(&bytes);
                    let mut plain = Term::new(
                        Config {
                            scrolling_history: 64,
                            osc52: Osc52::Disabled,
                            ..Config::default()
                        },
                        &Size(size),
                        VoidListener,
                    );
                    let mut parser: Processor = Processor::new();
                    parser.advance(&mut plain, &bytes);
                    let (a, b) = (probe.term.grid(), plain.grid());
                    for line in 0..6 {
                        for column in 0..24 {
                            let point = Point::new(Line(line), Column(column));
                            // Hyperlink ids are auto-generated from a global counter.
                            proptest::prop_assert_eq!(
                                restore::cell_key(&a[point]),
                                restore::cell_key(&b[point])
                            );
                        }
                    }
                    proptest::prop_assert_eq!(
                        restore::cursor_key(&a.cursor),
                        restore::cursor_key(&b.cursor)
                    );
                    proptest::prop_assert_eq!(
                        restore::cursor_key(&a.saved_cursor),
                        restore::cursor_key(&b.saved_cursor)
                    );
                    proptest::prop_assert_eq!(probe.term.mode(), plain.mode());
                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn mirror_follows_resize_and_deccolm() {
        let mut term = terminal();
        term.feed(b"\x1b[2;4r\x1b[3g\x1b[1;3H\x1bH");
        assert_eq!(term.side.scroll_region, (1, 4));
        term.resize(TermSize::new(24, 6).unwrap());
        assert_eq!(term.side.scroll_region, (1, 4), "unchanged size is a no-op");
        term.resize(TermSize::new(30, 8).unwrap());
        assert_eq!(term.side.scroll_region, (0, 8));
        assert!(term.side.tabs[2] && !term.side.tabs[8] && term.side.tabs[24]);
        term.feed(b"\x1b[2;4r\x1b[?3h");
        assert_eq!(term.side.scroll_region, (0, 8));
        term.feed(b"\x1b)0\x0e");
        assert_eq!(
            term.side.active_charset,
            alacritty_terminal::vte::ansi::CharsetIndex::G1
        );
        term.feed(b"\x1bc");
        let mut expected = tracked::MirroredState::new(30, 8);
        expected.risk = reconciliation::MODE | reconciliation::GEOMETRY;
        assert_eq!(term.side, expected);
    }

    #[test]
    fn spacer_combining_roundtrip_and_continuation() -> Result<(), Box<dyn std::error::Error>> {
        let size = TermSize::new(12, 4)?;
        for offset in 0..10 {
            let mut original = TerminalProbe::new(size, 64);
            original.feed("中e\u{301}\r\x1b[P\x1b[3b".as_bytes());
            if offset > 0 {
                original.feed(format!("\x1b[{offset}@").as_bytes());
            }
            let cell = &original.term.grid()[Point::new(Line(0), Column(offset))];
            assert!(cell.flags.contains(Flags::WIDE_CHAR_SPACER));
            assert_eq!(cell.zerowidth(), Some(&['\u{301}'; 3][..]));
            let sequence = original
                .restore_sequence()
                .map_err(|_| "unexpected recovery gap")?;
            assert!(
                sequence.approximations.is_empty(),
                "{:?}",
                sequence.approximations
            );
            let mut restored = TerminalProbe::new(size, 64);
            restored.feed(&sequence.bytes);
            assert_eq!(
                fingerprint_diff(
                    &original.restore_fingerprint(),
                    &restored.restore_fingerprint()
                ),
                None,
                "offset {offset}"
            );
            for suffix in [
                "\x1b[3b",
                "\x1b[2@",
                "\x1b[6n",
                "\x1b[?1049hX\x1b[?1049l",
                "\r\x1b[P",
            ] {
                original.feed(suffix.as_bytes());
                restored.feed(suffix.as_bytes());
                assert_eq!(
                    fingerprint_diff(
                        &original.restore_fingerprint(),
                        &restored.restore_fingerprint()
                    ),
                    None,
                    "offset {offset} suffix {suffix:?}"
                );
                assert_eq!(original.take_replies(), restored.take_replies());
            }
        }
        Ok(())
    }
    #[test]
    fn trailing_spacer_combining_is_explicitly_nonexact() -> Result<(), Box<dyn std::error::Error>>
    {
        let size = TermSize::new(12, 4)?;
        for offset in [10, 11] {
            let mut p = TerminalProbe::new(size, 64);
            p.feed("中e\u{301}\r\x1b[P\x1b[3b".as_bytes());
            p.feed(format!("\x1b[{offset}@").as_bytes());
            let seq = p
                .restore_sequence()
                .map_err(|_| "unexpected recovery gap")?;
            assert!(
                seq.approximations
                    .contains(&"combining data on a trailing wide-char spacer is not restored")
            );
        }
        Ok(())
    }

    #[test]
    fn clipped_wide_head_saved_wrap_never_loses_the_top_row()
    -> Result<(), Box<dyn std::error::Error>> {
        let size = TermSize::new(24, 6)?;
        let prefix = "a\x1b[6;22H中a\x1b7\x1b[4h\x1b[6;22H\x08中";
        for suffix in ["", "\x1b8", "\x1b8\x1b[?1049h"] {
            let mut original = TerminalProbe::new(size, 64);
            original.feed(prefix.as_bytes());
            original.feed(suffix.as_bytes());
            let before = original.restore_fingerprint();
            assert_eq!(
                original.restore_sequence(),
                Err(RestoreGap::ClippedWideHeadPendingWrap)
            );
            assert_eq!(
                original.restore_screen_bounded(1024 * 1024),
                Err(RestoreGap::ClippedWideHeadPendingWrap)
            );
            assert_eq!(
                original.repair_screen_bounded(1024 * 1024),
                Err(RestoreGap::ClippedWideHeadPendingWrap)
            );
            assert_eq!(
                fingerprint_diff(&before, &original.restore_fingerprint()),
                None
            );
        }
        // Saving the current non-wrap cursor clears just the unsafe saved slot.
        // Clipped wide cells and IRM themselves must remain recoverable.
        let mut original = TerminalProbe::new(size, 64);
        original.feed(prefix.as_bytes());
        original.feed(b"\x1b7");
        let sequence = original
            .restore_sequence()
            .map_err(|_| "unexpected cleared gap")?;
        assert!(sequence.approximations.is_empty());
        let mut restored = TerminalProbe::new(size, 64);
        restored.feed(&sequence.bytes);
        assert_eq!(
            fingerprint_diff(
                &original.restore_fingerprint(),
                &restored.restore_fingerprint()
            ),
            None
        );
        original.feed(b"\x1b[4lZ");
        restored.feed(b"\x1b[4lZ");
        assert_eq!(
            fingerprint_diff(
                &original.restore_fingerprint(),
                &restored.restore_fingerprint()
            ),
            None
        );
        Ok(())
    }

    #[test]
    fn restore_roundtrip_and_continuation_property() {
        let size = TermSize::new(24, 6).unwrap();
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        let cases = std::env::var("PULSE_RESTORE_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
            cases,
            ..Default::default()
        });
        let (exact, gap, approx, alt_active, alt_reentry) = (
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        );
        let alt_checked = AtomicUsize::new(0);
        let reasons = std::sync::Mutex::new(std::collections::BTreeMap::<&str, usize>::new());

        let strategy = (
            proptest::collection::vec(0..RESTORE_TOKENS.len(), 0..80),
            proptest::collection::vec(0..RESTORE_TOKENS.len(), 0..40),
        );
        runner
            .run(&strategy, |(prefix, suffix)| {
                let mut original = TerminalProbe::new(size, 64);
                original.feed(&tokens_to_bytes(&prefix));
                let Ok(restore) = original.restore_sequence() else {
                    gap.fetch_add(1, Relaxed);
                    return Ok(());
                };
                if let Ok(mut map) = reasons.lock() {
                    for reason in &restore.approximations {
                        *map.entry(reason).or_default() += 1;
                    }
                }
                if original.term.mode().contains(TermMode::ALT_SCREEN) {
                    alt_active.fetch_add(1, Relaxed);
                }
                if !restore.approximations.is_empty() {
                    approx.fetch_add(1, Relaxed);
                    return Ok(());
                }
                let mut restored = TerminalProbe::new(size, 64);
                restored.feed(&restore.bytes);
                original.take_replies();
                proptest::prop_assert!(
                    restored.take_replies().is_empty(),
                    "restore must not trigger replies"
                );
                if let Some(diff) = fingerprint_diff(
                    &original.restore_fingerprint(),
                    &restored.restore_fingerprint(),
                ) {
                    return Err(proptest::test_runner::TestCaseError::fail(format!(
                        "after restore: {diff}; prefix {:?}",
                        token_names(&prefix)
                    )));
                }
                let alt_entry = RESTORE_TOKENS.iter().position(|t| *t == "\x1b[?1049h");
                if suffix.iter().any(|&t| Some(t) == alt_entry) {
                    alt_reentry.fetch_add(1, Relaxed);
                }
                // REP before any printed character: vte has no preceding char and
                // cannot be reset to that state (documented gap).
                let rep = RESTORE_TOKENS.iter().position(|t| *t == "\x1b[3b");
                if original.side.last_input.is_none() && suffix.iter().any(|&t| Some(t) == rep) {
                    return Ok(());
                }
                let suffix_names = token_names(&suffix);
                let suffix = tokens_to_bytes(&suffix);
                original.feed(&suffix);
                restored.feed(&suffix);
                original.finish_input();
                restored.finish_input();
                if let Some(diff) = fingerprint_diff(
                    &original.restore_fingerprint(),
                    &restored.restore_fingerprint(),
                ) {
                    return Err(proptest::test_runner::TestCaseError::fail(format!(
                        "after continuation: {diff}; prefix {:?}",
                        token_names(&prefix)
                    )));
                }
                let queries = b"\x1b[6n\x1b[4$p\x1b[?6$p\x1b[?1002$p\x1b]4;1;?\x07";
                original.feed(queries);
                restored.feed(queries);
                proptest::prop_assert_eq!(
                    restored.take_replies(),
                    original.take_replies(),
                    "replies; suffix {:?}",
                    suffix_names
                );
                if original.term.mode().contains(TermMode::ALT_SCREEN) {
                    alt_checked.fetch_add(1, Relaxed);
                }
                exact.fetch_add(1, Relaxed);
                Ok(())
            })
            .unwrap();
        eprintln!(
            "restore property: exact={} gap={} approx={} alt-active={} (visible+continuation checked {}) alt-reentry={}",
            exact.load(Relaxed),
            gap.load(Relaxed),
            approx.load(Relaxed),
            alt_active.load(Relaxed),
            alt_checked.load(Relaxed),
            alt_reentry.load(Relaxed)
        );
        eprintln!(
            "approximation reasons (incl. alternate cases): {:?}",
            reasons.lock().map(|m| m.clone()).unwrap_or_default()
        );
    }

    #[test]
    fn inactive_grid_getter_is_non_destructive_and_tracks_resize() {
        let mut term = terminal();
        term.feed(b"PRIMARY\x1b[?1049hALT");
        let before = term.restore_fingerprint();
        for _ in 0..3 {
            assert_eq!(
                term.term.inactive_grid()[Point::new(Line(0), Column(0))].c,
                'P'
            );
        }
        assert_eq!(before, term.restore_fingerprint());
        term.resize(TermSize::new(18, 5).unwrap());
        assert_eq!(
            (
                term.term.inactive_grid().columns(),
                term.term.inactive_grid().screen_lines()
            ),
            (18, 5)
        );
        term.feed(b"\x1b[?1049l");
        assert_eq!(term.screen().cells[0][0].text, "P");
    }
    #[test]
    fn divergent_inactive_cursors_defer_instead_of_restoring_wrong_state() {
        let mut original = TerminalProbe::new(TermSize::new(32, 8).unwrap(), 128);
        original.feed(b"0123456789012345678901234567890123456789\r\nSECOND\x1b[?1049hALT");
        original.resize(TermSize::new(44, 12).unwrap());
        let before = original.restore_fingerprint();
        assert_eq!(
            original.restore_sequence(),
            Err(RestoreGap::InactiveCursorDivergence)
        );
        assert_eq!(before, original.restore_fingerprint());
        original.feed(b"\x1b[?1049l");
        let restore = original.restore_sequence().unwrap();
        let mut restored = TerminalProbe::new(TermSize::new(44, 12).unwrap(), 128);
        restored.feed(&restore.bytes);
        assert_eq!(
            original.restore_fingerprint(),
            restored.restore_fingerprint()
        );
    }

    #[test]
    fn restore_defers_hidden_nonedge_wrap_without_corrupting_source() {
        let mut original = terminal();
        original.feed(b"123456789012345678901234\x1b[2Z\x1b[?1049ha");
        let before = original.restore_fingerprint();
        assert_eq!(
            original.restore_sequence().unwrap_err(),
            RestoreGap::NonEdgePendingWrap
        );
        assert_eq!(original.restore_fingerprint(), before);
        original.feed(b"\x1b[?1049l\r\x1b7");
        let frame = original.restore_sequence().unwrap();
        let mut restored = terminal();
        restored.feed(&frame.bytes);
        assert_eq!(
            restored.restore_fingerprint(),
            original.restore_fingerprint()
        );
    }
    #[test]
    fn dual_buffer_restore_preserves_hidden_main_and_latent_cursor() {
        for (prefix, suffix) in [
            (
                "PRIMARY 中\x1b[31m\x1b7\x1b[?1049hALT",
                "\x1b[?1049l!\x1b8?",
            ),
            (
                "MAIN\x1b[?1049h\x1b[3;4H\x1b[34m\x1b7\x1b[?1049l",
                "\x1b[?1049h\x1b8ALT\x1b[?1049l!",
            ),
            ("\x1b]8;;http://x\x1b\\\x1b[?1049h\r", "X\x1b[?1049lY"),
        ] {
            for resized in [
                None,
                Some(TermSize::new(20, 5).unwrap()),
                Some(TermSize::new(30, 10).unwrap()),
            ] {
                let mut source = terminal();
                source.feed(prefix.as_bytes());
                if let Some(size) = resized {
                    source.resize(size);
                }
                let size = resized.unwrap_or(TermSize::new(24, 6).unwrap());
                let before = source.restore_fingerprint();
                let restore = source.restore_sequence().unwrap();
                assert!(!restore.main_screen_missing);
                assert!(
                    restore.approximations.is_empty(),
                    "{:?}",
                    restore.approximations
                );
                assert_eq!(
                    before,
                    source.restore_fingerprint(),
                    "serializer mutated source"
                );
                let mut restored = TerminalProbe::new(size, 64);
                restored.feed(&restore.bytes);
                assert_eq!(restored.restore_fingerprint(), before);
                source.take_replies();
                assert!(restored.take_replies().is_empty());
                source.feed(suffix.as_bytes());
                restored.feed(suffix.as_bytes());
                assert_eq!(restored.restore_fingerprint(), source.restore_fingerprint());
            }
        }
    }

    #[test]
    fn restore_reports_sync_and_pending_gaps() {
        let mut term = terminal();
        term.feed(b"\x1b[?2026hbuffered");
        assert_eq!(term.restore_sequence(), Err(RestoreGap::SyncPending));
        term.feed(b"\x1b[?2026l");
        assert!(term.restore_sequence().is_ok());
        term.feed(b"main\x1b[?1049halt");
        assert!(!term.restore_sequence().unwrap().main_screen_missing);
        term.feed(b"\x1b]52;c;");
        term.feed(&vec![b'A'; ground::MAX_PENDING]);
        assert_eq!(
            term.restore_sequence(),
            Err(RestoreGap::PendingSequenceTooLong)
        );
    }
}
