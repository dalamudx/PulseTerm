//! Evaluation spike: serialize the authoritative terminal into VT bytes that
//! rebuild an equivalent state in a fresh parser (alacritty or xterm.js), using
//! only public alacritty_terminal 0.26 API plus the transparent mirror.
//! Coverage and known gaps: docs/architecture/terminal-restore-evaluation.md.

use std::fmt::Write;

#[cfg(test)]
#[path = "restore_budget_tests.rs"]
mod budget_tests;

use alacritty_terminal::grid::{Cursor, Dimensions, Grid};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{
    CharsetIndex, Color, CursorShape, CursorStyle, StandardCharset,
};

use crate::{TerminalProbe, sgr_color};

/// Output buffer that also remembers the last printed character: vte keeps it
/// privately for REP (`CSI b`), so the restore must end with the right print.
struct Emitter {
    buf: String,
    last_print: Option<char>,
    limit: usize,
    exceeded: bool,
}

impl Write for Emitter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.push_str(s);
        if self.exceeded {
            Err(std::fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl Emitter {
    fn push_str(&mut self, s: &str) {
        if self.exceeded || s.len() > self.limit.saturating_sub(self.buf.len()) {
            self.exceeded = true;
        } else {
            self.buf.push_str(s);
        }
    }
    fn push(&mut self, c: char) {
        let mut bytes = [0; 4];
        self.push_str(c.encode_utf8(&mut bytes));
    }
    /// A character that goes through the parser's print path.
    fn print(&mut self, c: char) {
        self.push(c);
        self.last_print = Some(c);
    }
}

/// A restore cannot be produced right now; retry when the condition clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreGap {
    /// Serialization exceeded the byte budget; no partial replay is returned.
    OutputTooLarge,
    /// A synchronized update (DEC 2026) is buffered and not yet applied.
    SyncPending,
    /// An unfinished escape sequence is longer than the replay bound.
    PendingSequenceTooLong,
    /// Resize separated the hidden primary current/saved cursors; 1049 would
    /// overwrite one during reconstruction. Retry after leaving alternate screen.
    InactiveCursorDivergence,
    /// A pending-wrap bit away from the right edge has no direct VT setter.
    NonEdgePendingWrap,
}

/// VT bytes plus the parts that could not be expressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restore {
    /// Feed into a freshly created terminal of the same size.
    pub bytes: Vec<u8>,
    /// Compatibility diagnostic; always false with the approved inactive getter.
    /// Unrepresentable hidden cursor states return RestoreGap, never blank data.
    pub main_screen_missing: bool,
    /// States VT sequences cannot express exactly; the restore is approximate.
    pub approximations: Vec<&'static str>,
}

impl TerminalProbe {
    /// Build a complete-state restore (evaluation spike, visible area only).
    pub fn restore_sequence(&self) -> Result<Restore, RestoreGap> {
        self.restore_sequence_bounded(1024 * 1024)
    }

    /// Generate within a payload byte budget, returning no partial bytes on error.
    /// This limits emitted content, not terminal/grid or allocator overhead.
    pub fn restore_sequence_bounded(&self, limit: usize) -> Result<Restore, RestoreGap> {
        use alacritty_terminal::vte::ansi::Timeout;
        if self.parser.sync_timeout().pending_timeout() || self.parser.sync_bytes_count() > 0 {
            return Err(RestoreGap::SyncPending);
        }
        let pending = self
            .ground
            .pending()
            .ok_or(RestoreGap::PendingSequenceTooLong)?;
        let term = &self.term;
        let grid = term.grid();
        let mode = *term.mode();
        let mut out = Emitter {
            buf: String::new(),
            last_print: None,
            limit,
            exceeded: false,
        };
        out.push_str("\x1bc");

        for index in 0..=258 {
            if let Some(rgb) = term.colors()[index] {
                let target = match index {
                    256 => "10".to_string(),
                    257 => "11".to_string(),
                    258 => "12".to_string(),
                    _ => format!("4;{index}"),
                };
                let _ = write!(
                    out,
                    "\x1b]{target};rgb:{:02x}/{:02x}/{:02x}\x07",
                    rgb.r, rgb.g, rgb.b
                );
            }
        }

        let mut approximations = Vec::new();
        let alternate = mode.contains(TermMode::ALT_SCREEN);
        let inactive = term.inactive_grid();
        let nonedge = |cursor: &alacritty_terminal::grid::Cursor<Cell>| {
            cursor.input_needs_wrap && cursor.point.column.0 + 1 != term.columns()
        };
        if nonedge(&grid.cursor)
            || nonedge(&grid.saved_cursor)
            || nonedge(&inactive.saved_cursor)
            || (alternate && nonedge(&inactive.cursor))
        {
            return Err(RestoreGap::NonEdgePendingWrap);
        }
        if alternate {
            if cursor_key(&inactive.cursor) != cursor_key(&inactive.saved_cursor) {
                return Err(RestoreGap::InactiveCursorDivergence);
            }
            // Restore the hidden primary screen before entering the active alternate.
            // Entering 1049 saves its cursor without any destructive inspection.
            write_grid(&mut out, inactive, &mut approximations);
            write_tab_cells(&mut out, inactive);
            // xterm stores scroll margins per buffer; alacritty's margins are
            // global. Seed hidden primary margins too, or returning from alt
            // makes subsequent IL/DL/SU/SD use a full-screen region in xterm.
            let (top, bottom) = self.side.scroll_region;
            if (top, bottom) != (0, inactive.screen_lines() as i32) {
                let _ = write!(out, "\x1b[{};{}r", top + 1, bottom);
            }
            place_cursor(&mut out, inactive, &inactive.cursor, 0);
            write_template(&mut out, &inactive.cursor.template);
            designate(&mut out, &inactive.cursor);

            out.push_str("\x1b[?1049h\x1b(B\x1b)B\x1b*B\x1b+B\x0f");
        } else {
            // Entering alternate clears its content and replaces its current cursor,
            // but preserves its saved cursor. Rebuild that slot for future re-entry.
            out.push_str("\x1b[?1049h");
            place_cursor(&mut out, inactive, &inactive.saved_cursor, 0);
            write_template(&mut out, &inactive.saved_cursor.template);
            designate(&mut out, &inactive.saved_cursor);
            out.push_str("\x1b7\x1b[?1049l\x1b(B\x1b)B\x1b*B\x1b+B\x0f");
        }
        write_grid(&mut out, grid, &mut approximations);

        // REP state on the alternate screen (no scratch screen available): rewrite,
        // last, one plain cell whose final printed character is the preceding char.
        if let (Some(c), true) = (self.side.last_input, alternate) {
            'search: for line in (0..grid.screen_lines() as i32).rev() {
                for column in (0..grid.columns()).rev() {
                    let row = &grid[Line(line)];
                    let cell = &row[Column(column)];
                    let plain = !cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        && (!cell.flags.contains(Flags::WIDE_CHAR)
                            || (column + 1 < grid.columns()
                                && row[Column(column + 1)]
                                    .flags
                                    .contains(Flags::WIDE_CHAR_SPACER)));
                    let printed = cell
                        .zerowidth()
                        .and_then(|zero| zero.last().copied())
                        .unwrap_or(if cell.c == '\t' { ' ' } else { cell.c });
                    if plain && printed == c {
                        let _ = write!(out, "\x1b[{};{}H", line + 1, column + 1);
                        write_cell(&mut out, cell);
                        break 'search;
                    }
                }
            }
        }

        write_tab_cells(&mut out, grid);

        out.push_str("\x1b[3g");
        for (column, _) in self.side.tabs.iter().enumerate().filter(|(_, stop)| **stop) {
            let _ = write!(out, "\x1b[1;{}H\x1bH", column + 1);
        }
        let (top, bottom) = self.side.scroll_region;
        if (top, bottom) != (0, grid.screen_lines() as i32) {
            let _ = write!(out, "\x1b[{};{}r", top + 1, bottom);
        }

        // Saved cursor with its pen, charsets and pending-wrap flag. On the primary
        // screen, entering 1049 performs the save (like DECSC in both parsers) and
        // gives a scratch screen to print the REP preceding character; leaving it
        // returns to the same cursor without touching visible cells.
        place_cursor(&mut out, grid, &grid.saved_cursor, 0);
        write_template(&mut out, &grid.saved_cursor.template);
        designate(&mut out, &grid.saved_cursor);
        match (self.side.last_input, alternate) {
            (Some(c), false) => {
                out.push_str("\x1b[?1049h");
                out.print(c);
                out.push_str("\x1b[?1049l");
            }
            _ => out.push_str("\x1b7"),
        }
        out.push_str("\x1b]8;;\x1b\\");

        // Current cursor; cells are rewritten before non-ASCII charsets are active.
        out.push_str("\x1b(B\x1b)B\x1b*B\x1b+B\x0f");
        for (name, cursor) in [("saved", &grid.saved_cursor), ("current", &grid.cursor)] {
            let wide_tail = grid[cursor.point].flags.contains(Flags::WIDE_CHAR_SPACER);
            if cursor.input_needs_wrap && cursor.point.column.0 + 1 != grid.columns() && !wide_tail
            {
                // alacritty keeps the flag across CBT/LF; VT can only set it at the margin.
                approximations.push(if name == "saved" {
                    "saved cursor: pending wrap away from the right margin"
                } else {
                    "cursor: pending wrap away from the right margin"
                });
            }
        }
        let line = grid.cursor.point.line.0;
        if mode.contains(TermMode::ORIGIN) && !(top..bottom).contains(&line) {
            // Only DECRC can put the cursor outside the region under DECOM (CUP is
            // relative and clamped). This consumes the DECSC slot.
            place_cursor(&mut out, grid, &grid.cursor, 0);
            write_template(&mut out, &grid.cursor.template);
            designate(&mut out, &grid.cursor);
            out.push_str("\x1b7\x1b[?6h\x1b8");
            if grid.saved_cursor != grid.cursor {
                approximations.push("origin-mode cursor outside region: saved cursor overwritten");
            }
        } else {
            let origin_top = if mode.contains(TermMode::ORIGIN) {
                out.push_str("\x1b[?6h");
                top
            } else {
                0
            };
            place_cursor(&mut out, grid, &grid.cursor, origin_top);
            write_template(&mut out, &grid.cursor.template);
            designate(&mut out, &grid.cursor);
        }
        if self.side.active_charset == CharsetIndex::G1 {
            out.push('\x0e');
        }

        for (flag, code) in [
            (TermMode::APP_CURSOR, 1),
            (TermMode::LINE_WRAP, 7),
            (TermMode::SHOW_CURSOR, 25),
            (TermMode::FOCUS_IN_OUT, 1004),
            (TermMode::UTF8_MOUSE, 1005),
            (TermMode::SGR_MOUSE, 1006),
            (TermMode::ALTERNATE_SCROLL, 1007),
            (TermMode::URGENCY_HINTS, 1042),
            (TermMode::BRACKETED_PASTE, 2004),
        ] {
            let _ = write!(
                out,
                "\x1b[?{code}{}",
                if mode.contains(flag) { 'h' } else { 'l' }
            );
        }
        if mode.contains(TermMode::MOUSE_MOTION) {
            out.push_str("\x1b[?1003h");
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            out.push_str("\x1b[?1002h");
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            out.push_str("\x1b[?1000h");
        }
        out.push_str(if mode.contains(TermMode::APP_KEYPAD) {
            "\x1b="
        } else {
            "\x1b>"
        });
        let _ = write!(
            out,
            "\x1b[4{}",
            if mode.contains(TermMode::INSERT) {
                'h'
            } else {
                'l'
            }
        );
        let _ = write!(
            out,
            "\x1b[20{}",
            if mode.contains(TermMode::LINE_FEED_NEW_LINE) {
                'h'
            } else {
                'l'
            }
        );

        let style = term.cursor_style();
        if style != CursorStyle::default() {
            let base = match style.shape {
                CursorShape::Block => Some(1),
                CursorShape::Underline => Some(3),
                CursorShape::Beam => Some(5),
                CursorShape::HollowBlock | CursorShape::Hidden => None,
            };
            if let Some(base) = base {
                let _ = write!(out, "\x1b[{} q", base + u8::from(!style.blinking));
            }
        }

        if self.side.last_input.is_some() && out.last_print != self.side.last_input {
            approximations.push("REP preceding character");
        }
        if out.exceeded
            || pending.len().saturating_add(self.utf8_tail.len())
                > limit.saturating_sub(out.buf.len())
        {
            return Err(RestoreGap::OutputTooLarge);
        }
        let mut bytes = out.buf.into_bytes();
        bytes.extend_from_slice(pending);
        bytes.extend_from_slice(&self.utf8_tail);
        Ok(Restore {
            bytes,
            main_screen_missing: false,
            approximations,
        })
    }

    /// Structural fingerprint of every state the restore claims to reproduce
    /// (visible cells, both cursors, modes, mirrored state, colours, style).
    pub fn restore_fingerprint(&self) -> String {
        let term = &self.term;
        let grid = term.grid();
        let mut out = String::new();
        for line in 0..grid.screen_lines() as i32 {
            for column in 0..grid.columns() {
                let cell = &grid[Point::new(Line(line), Column(column))];
                let _ = write!(out, "{}|", cell_key(cell));
            }
            out.push('\n');
        }
        for (name, cursor) in [("cursor", &grid.cursor), ("saved", &grid.saved_cursor)] {
            let _ = writeln!(
                out,
                "{name}: {:?} wrap={} charsets={:?} pen={}",
                cursor.point,
                cursor.input_needs_wrap,
                cursor.charsets,
                cell_key(&cursor.template)
            );
        }
        let inactive = term.inactive_grid();
        if term.mode().contains(TermMode::ALT_SCREEN) {
            // Hidden primary contents and its cursor become visible on 1049l.
            out.push_str("inactive-primary:\n");
            for line in 0..inactive.screen_lines() as i32 {
                for column in 0..inactive.columns() {
                    let _ = write!(
                        out,
                        "{}|",
                        cell_key(&inactive[Point::new(Line(line), Column(column))])
                    );
                }
                out.push('\n');
            }
            let _ = writeln!(out, "inactive-cursor: {}", cursor_key(&inactive.cursor));
        }
        // Alternate content/current cursor is overwritten on next entry, but its
        // saved cursor survives. Both active states must compare this latent slot.
        let _ = writeln!(
            out,
            "inactive-saved: {}",
            cursor_key(&inactive.saved_cursor)
        );
        let _ = writeln!(out, "mode: {:?}", *term.mode() - TermMode::VI);
        // REP state is verified behaviourally (REP in the continuation), because a
        // fresh vte parser can never return to "no preceding character".
        let side = &self.side;
        let _ = writeln!(
            out,
            "mirror: {:?} {:?} {:?}",
            side.scroll_region, side.active_charset, side.tabs
        );
        let _ = writeln!(out, "style: {:?}", term.cursor_style());
        for index in 0..alacritty_terminal::term::color::COUNT {
            if let Some(rgb) = term.colors()[index] {
                let _ = write!(out, "{index}={rgb:?} ");
            }
        }
        out
    }
}

pub(crate) fn cursor_key(cursor: &Cursor<Cell>) -> String {
    format!(
        "{:?} wrap={} charsets={:?} pen={}",
        cursor.point,
        cursor.input_needs_wrap,
        cursor.charsets,
        cell_key(&cursor.template)
    )
}

pub(crate) fn cell_key(cell: &Cell) -> String {
    let mut key = format!("{:?}", cell.c);
    if let Some(zero) = cell.zerowidth() {
        key.extend(zero);
    }
    // Soft-wrap markers are not restored by the spike (affect reflow only).
    let flags = cell.flags - Flags::WRAPLINE - Flags::LEADING_WIDE_CHAR_SPACER;
    let link = cell.hyperlink().map(|link| link.uri().to_owned());
    format!(
        "{key}/{:?}/{:?}/{flags:?}/{:?}/{link:?}",
        cell.fg,
        cell.bg,
        cell.underline_color()
    )
}

/// SGR reset followed by every attribute of `cell` (used for cells and pens).
fn write_grid(out: &mut Emitter, grid: &Grid<Cell>, approximations: &mut Vec<&'static str>) {
    for line in 0..grid.screen_lines() as i32 {
        for column in 0..grid.columns() {
            if out.exceeded {
                return;
            }
            let row = &grid[Line(line)];
            let cell = &row[Column(column)];
            let previous_wide =
                column > 0 && row[Column(column - 1)].flags.contains(Flags::WIDE_CHAR);
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                if previous_wide {
                    continue;
                }
                // Orphan spacer (left over by EL/ECH/DCH/ICH): create a pair with
                // the spacer's pen and remove its head without touching cells
                // already written on the left.
                let mut head = cell.clone();
                head.c = '\u{4e2d}';
                head.flags.remove(Flags::WIDE_CHAR_SPACER);
                head.flags.insert(Flags::WIDE_CHAR);
                if column + 1 < grid.columns() {
                    // Pair at c..c+1, then DCH shifts the spacer into c. Cells on the
                    // right are not written yet.
                    let _ = write!(out, "\x1b[{};{}H", line + 1, column + 1);
                    write_cell(out, &head);
                    let _ = write!(out, "\x1b[{};{}H\x1b[P", line + 1, column + 1);
                } else if !row[Column(column - 1)]
                    .flags
                    .contains(Flags::WIDE_CHAR_SPACER)
                {
                    // Last column: pair at c-1..c, erase the head, rewrite c-1.
                    let _ = write!(out, "\x1b[{};{}H", line + 1, column);
                    write_cell(out, &head);
                    let _ = write!(out, "\x1b[{};{}H\x1b[X", line + 1, column);
                    write_cell(out, &row[Column(column - 1)]);
                } else {
                    approximations
                        .push("orphan wide-char spacer after a spacer in the last column");
                }
                continue;
            }
            if cell.flags.contains(Flags::WIDE_CHAR) && column + 1 == grid.columns() {
                // Wide char in the last column without spacer (shifted by ICH/IRM):
                // a normal write would wrap. Write the pair one column earlier and
                // shift it right with ICH, then restore the left neighbour.
                if column > 0
                    && !row[Column(column - 1)]
                        .flags
                        .contains(Flags::WIDE_CHAR_SPACER)
                {
                    let _ = write!(out, "\x1b[{};{}H", line + 1, column);
                    write_cell(out, cell);
                    let _ = write!(out, "\x1b[{};{}H\x1b[@", line + 1, column);
                    write_cell(out, &row[Column(column - 1)]);
                } else {
                    approximations.push("wide char in the last column after a spacer");
                }
                continue;
            }
            let _ = write!(out, "\x1b[{};{}H", line + 1, column + 1);
            write_cell(out, cell);
            let next_is_spacer = column + 1 < grid.columns()
                && row[Column(column + 1)]
                    .flags
                    .contains(Flags::WIDE_CHAR_SPACER);
            if cell.flags.contains(Flags::WIDE_CHAR)
                && !next_is_spacer
                && column + 1 < grid.columns()
            {
                // Broken wide char (its spacer was erased): drop the new spacer.
                let _ = write!(out, "\x1b[{};{}H\x1b[X", line + 1, column + 2);
            }
        }
    }
}

fn write_tab_cells(out: &mut Emitter, grid: &Grid<Cell>) {
    // Copy-paste tab markers: HT stores '\t' in a blank start cell (even a wide
    // char spacer) and keeps its attributes. Default tab stops are active here.
    for line in 0..grid.screen_lines() as i32 {
        for column in 0..grid.columns() {
            if grid[Line(line)][Column(column)].c == '\t' {
                let _ = write!(out, "\x1b[{};{}H\t", line + 1, column + 1);
            }
        }
    }
}

fn write_pen(out: &mut Emitter, cell: &Cell) {
    out.push_str("\x1b[0m");
    sgr_color(&mut out.buf, cell.fg, true);
    sgr_color(&mut out.buf, cell.bg, false);
    match cell.underline_color() {
        Some(Color::Spec(rgb)) => {
            let _ = write!(out, "\x1b[58;2;{};{};{}m", rgb.r, rgb.g, rgb.b);
        }
        Some(Color::Indexed(index)) => {
            let _ = write!(out, "\x1b[58;5;{index}m");
        }
        Some(Color::Named(named)) => {
            let _ = write!(out, "\x1b[58;5;{}m", named as usize);
        }
        None => {}
    }
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::UNDERLINE, "4"),
        (Flags::DOUBLE_UNDERLINE, "4:2"),
        (Flags::UNDERCURL, "4:3"),
        (Flags::DOTTED_UNDERLINE, "4:4"),
        (Flags::DASHED_UNDERLINE, "4:5"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if cell.flags.contains(flag) {
            let _ = write!(out, "\x1b[{code}m");
        }
    }
}

/// Pen of a cursor, including an open hyperlink (OSC 8) that new cells inherit.
fn write_template(out: &mut Emitter, template: &Cell) {
    out.push_str("\x1b]8;;\x1b\\");
    write_pen(out, template);
    if let Some(link) = template.hyperlink() {
        let _ = write!(out, "\x1b]8;id={};{}\x1b\\", link.id(), link.uri());
    }
}

fn write_cell(out: &mut Emitter, cell: &Cell) {
    // Cursor restoration may leave an inherited hyperlink open. Cell rendering
    // must establish its own link rather than leak that pen into blank cells.
    out.push_str("\x1b]8;;\x1b\\");
    write_pen(out, cell);
    let link = cell.hyperlink();
    if let Some(link) = &link {
        let _ = write!(out, "\x1b]8;id={};{}\x1b\\", link.id(), link.uri());
    }
    // A literal tab stored by the parser is a blank physical cell, not an HT action.
    out.print(if cell.c == '\t' { ' ' } else { cell.c });
    for &zero in cell.zerowidth().into_iter().flatten() {
        out.print(zero);
    }
    if link.is_some() {
        out.push_str("\x1b]8;;\x1b\\");
    }
}

/// Move to `cursor`; a pending wrap is recreated by rewriting the last cell.
fn place_cursor(out: &mut Emitter, grid: &Grid<Cell>, cursor: &Cursor<Cell>, origin_top: i32) {
    let point = cursor.point;
    let mut column = point.column.0;
    if cursor.input_needs_wrap && grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) && column > 0
    {
        column -= 1;
    }
    let _ = write!(
        out,
        "\x1b[{};{}H",
        point.line.0 - origin_top + 1,
        column + 1
    );
    if cursor.input_needs_wrap {
        write_cell(out, &grid[Point::new(point.line, Column(column))]);
    }
}

fn designate(out: &mut Emitter, cursor: &Cursor<Cell>) {
    for (index, intro) in [
        (CharsetIndex::G0, '('),
        (CharsetIndex::G1, ')'),
        (CharsetIndex::G2, '*'),
        (CharsetIndex::G3, '+'),
    ] {
        let code = match cursor.charsets[index] {
            StandardCharset::Ascii => 'B',
            StandardCharset::SpecialCharacterAndLineDrawing => '0',
        };
        let _ = write!(out, "\x1b{intro}{code}");
    }
}
