//! Sans-I/O terminal inspection and an intentionally limited screen-replay probe.
//! This is not yet a production snapshot: inactive buffers, saved cursors, scroll
//! regions, tab stops, pending wrap and parser state are not restored.
#![forbid(unsafe_code)]

use std::fmt::Write;
use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
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
struct ReplyListener(Sender<String>);
impl EventListener for ReplyListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(reply) = event {
            let _ = self.0.send(reply);
        }
        // No clipboard, title, color or size requests are serviced by this probe.
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
    parser: Processor,
    replies: Receiver<String>,
    utf8_tail: Vec<u8>,
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
            term: Term::new(config, &Size(size), ReplyListener(sender)),
            parser: Processor::new(),
            replies,
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
    /// Repeated calls are harmless. This does not reset or serialize ANSI parser state.
    pub fn finish_input(&mut self) {
        if !self.utf8_tail.is_empty() {
            self.utf8_tail.clear();
            self.parser.advance(&mut self.term, "\u{fffd}".as_bytes());
        }
    }

    /// Drain authoritative protocol replies; a transport would write these upstream.
    pub fn take_replies(&self) -> Vec<String> {
        self.replies.try_iter().collect()
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
    fn feed_complete_prefix(&mut self, bytes: &[u8]) {
        let end = complete_utf8_prefix(bytes);
        self.parser.advance(&mut self.term, &bytes[..end]);
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

fn sgr_color(output: &mut String, color: Color, foreground: bool) {
    let channel = if foreground { 38 } else { 48 };
    match color {
        Color::Named(NamedColor::Foreground | NamedColor::Background) => {}
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
    fn history_is_bounded_without_an_attached_view() {
        let mut source = terminal();
        for _ in 0..100_000 {
            source.feed(b"sample log line\r\n");
        }
        assert_eq!(source.history_lines(), 64);
        assert!(source.grid_cell_bytes() > 0);
    }
}
