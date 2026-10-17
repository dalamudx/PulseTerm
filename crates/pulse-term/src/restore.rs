//! Evaluation spike: serialize the authoritative terminal into VT bytes that
//! rebuild an equivalent state in a fresh parser (alacritty or xterm.js), using
//! only public alacritty_terminal 0.26 API plus the transparent mirror.
//! Coverage and known gaps: docs/architecture/terminal-restore-evaluation.md.

use std::fmt::Write;

// Reset only parser/view state, NOT primary scrollback. Include CAN for incomplete
// sequences, palette overrides, input modes, charsets and the viewport. This is
// the explicitly supported mode subset (kitty stacks/title stack remain outside
// the recovery contract). Screen contents are restored by write_grid below.
const SCREEN_RESET: &str = "\x18\x1b[?1049l\x1b[?6l\x1b[r\x1b[4l\x1b[20l\x1b[?7h\x1b[?25h\x1b[?1l\x1b>\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1005l\x1b[?1006l\x1b[?1007h\x1b[?1042h\x1b[?2004l\x1b(B\x1b)B\x1b*B\x1b+B\x0f\x1b[3g\x1b[0m\x1b]8;;\x1b\\\x1b]104\x1b\\\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\";

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
pub(crate) struct Emitter {
    pub(crate) buf: String,
    pub(crate) last_print: Option<char>,
    pub(crate) limit: usize,
    pub(crate) exceeded: bool,
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
    pub(crate) fn push_str(&mut self, s: &str) {
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
    pub(crate) fn print(&mut self, c: char) {
        self.push(c);
        self.last_print = Some(c);
    }
}

/// A restore cannot be produced right now; retry when the condition clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreGap {
    /// Recent-history row contains a non-encodable wide-cell anomaly.
    HistoryUnsupported,
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
    /// A pending-wrap cursor points at a clipped wide head on the right edge.
    /// Reprinting that head would wrap/scroll instead of recreating the same cursor.
    ClippedWideHeadPendingWrap,
}

/// VT bytes plus the parts that could not be expressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restore {
    /// Feed into a freshly created terminal of the same size.
    pub bytes: Vec<u8>,
    /// Set renderer base theme BEFORE replay. These are not remote OSC overrides;
    /// OSC resets and RIS must continue to resolve to this same base.
    pub palette: crate::QueryPalette,
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
        self.build_restore(limit, false, false)
    }

    /// Rebuild visible/buffer state without RIS, preserving primary scrollback.
    /// Abort/reset the old decoder before feeding this replay; reject approximate
    /// frames. History can be seeded into a fresh renderer before this operation.
    /// Not atomic or a production attach protocol by itself.
    pub fn restore_screen_bounded(&self, limit: usize) -> Result<Restore, RestoreGap> {
        self.build_restore(limit, true, false)
    }

    /// Screen repair wrapped in synchronized updates; end precedes incomplete parser tail.
    pub fn repair_screen_bounded(&self, limit: usize) -> Result<Restore, RestoreGap> {
        self.build_restore(limit, true, true)
    }

    fn build_restore(
        &self,
        limit: usize,
        preserve_history: bool,
        synchronized: bool,
    ) -> Result<Restore, RestoreGap> {
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
        if synchronized {
            out.push_str("\x18\x1b[?2026h");
        }
        if preserve_history {
            out.push_str(SCREEN_RESET);
            write_stops(&mut out, (0..term.columns()).step_by(8));
        } else {
            out.push_str("\x1bc");
        }

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
        let clipped_wrap = |grid: &Grid<Cell>, cursor: &Cursor<Cell>| {
            cursor.input_needs_wrap
                && cursor.point.column.0 + 1 == grid.columns()
                && grid[cursor.point].flags.contains(Flags::WIDE_CHAR)
        };
        if clipped_wrap(grid, &grid.cursor)
            || clipped_wrap(grid, &grid.saved_cursor)
            || clipped_wrap(inactive, &inactive.saved_cursor)
            || (alternate && clipped_wrap(inactive, &inactive.cursor))
        {
            return Err(RestoreGap::ClippedWideHeadPendingWrap);
        }
        if alternate {
            if cursor_key(&inactive.cursor) != cursor_key(&inactive.saved_cursor) {
                return Err(RestoreGap::InactiveCursorDivergence);
            }
            // Restore the hidden primary screen before entering the active alternate.
            // Entering 1049 saves its cursor without any destructive inspection.
            write_grid(&mut out, inactive, &mut approximations);
            write_tab_cells(&mut out, inactive);
            write_stops(
                &mut out,
                self.side
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, set)| **set)
                    .map(|(i, _)| i),
            );
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
            write_stops(&mut out, (0..term.columns()).step_by(8));
        } else {
            // Entering alternate clears its content and replaces its current cursor,
            // but preserves its saved cursor. Rebuild that slot for future re-entry.
            out.push_str("\x1b[?1049h");
            write_stops(
                &mut out,
                self.side
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, set)| **set)
                    .map(|(i, _)| i),
            );
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

        write_stops(
            &mut out,
            self.side
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, set)| **set)
                .map(|(i, _)| i),
        );
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
            approximations.push("origin-mode cursor outside region is not representable by xterm");
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
        if synchronized {
            out.push_str("\x1b[?2026l");
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
            palette: self.palette.clone(),
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
        let _ = writeln!(out, "base palette: {:?}", self.palette);
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
    // Cell keys omit row metadata; wrap continuity is checked separately.
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
    // ED2 pushes the primary viewport into alacritty history, even if blank.
    // Erase each row instead: recovery must not manufacture scrollback.
    // Entering alt can inherit Alacritty's global partial scroll region from
    // hidden-primary setup. Draw with full margins before natural autowrap;
    // each buffer's source margins are restored after its cells are drawn.
    out.push_str("\x1b[r");
    out.push_str("\x1b[0m\x1b]8;;\x1b\\");
    let history_continuation = grid.history_size() > 0
        && grid[Line(-1)][Column(grid.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE);
    for line in 1..=grid.screen_lines() {
        if line == 1 && history_continuation {
            // xterm EL2 clears isWrapped on this row. ECH clears the same cells
            // while preserving the soft continuation seeded by history replay.
            let _ = write!(out, "\x1b[1;1H\x1b[{}X", grid.columns());
        } else {
            let _ = write!(out, "\x1b[{line};1H\x1b[2K");
        }
    }
    out.push_str("\x1b[H");
    let blank = Cell::default();
    let mut pen: Option<Cell> = None;
    let mut pending_soft_wrap = false;
    for line in 0..grid.screen_lines() as i32 {
        let mut next_column = pending_soft_wrap.then_some(0);
        // Anomaly repair via DCH/ICH/ECH can recolour its vacated trailing cells.
        // Do not skip defaults on those rows: rewrite the whole tail explicitly.
        let row = &grid[Line(line)];
        let anomalous = (0..grid.columns()).any(|c| {
            let flags = row[Column(c)].flags;
            (flags.contains(Flags::WIDE_CHAR_SPACER)
                && (c == 0 || !row[Column(c - 1)].flags.contains(Flags::WIDE_CHAR)))
                || (flags.contains(Flags::WIDE_CHAR)
                    && (c + 1 == grid.columns()
                        || !row[Column(c + 1)].flags.contains(Flags::WIDE_CHAR_SPACER)))
        });
        let mut end = grid.columns();
        while !anomalous && end > 0 && row[Column(end - 1)] == blank {
            end -= 1;
        }
        if pending_soft_wrap && end == 0 {
            approximations.push("empty soft-wrap continuation cannot be reproduced exactly");
        }
        for column in 0..end {
            if out.exceeded {
                return;
            }
            let row = &grid[Line(line)];
            let cell = &row[Column(column)];
            let previous_wide =
                column > 0 && row[Column(column - 1)].flags.contains(Flags::WIDE_CHAR);
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                if previous_wide {
                    write_spacer_marks(out, cell, line, column, grid.columns(), approximations);
                    if cell.zerowidth().is_some_and(|zero| !zero.is_empty()) {
                        next_column = None;
                        pen = None;
                    }
                    continue;
                }
                next_column = None;
                pen = None;
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
                write_spacer_marks(out, cell, line, column, grid.columns(), approximations);
                continue;
            }
            if cell.flags.contains(Flags::WIDE_CHAR) && column + 1 == grid.columns() {
                next_column = None;
                pen = None;
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
            if next_column != Some(column) {
                let _ = write!(out, "\x1b[{};{}H", line + 1, column + 1);
            }
            if pen.as_ref().is_some_and(|old| same_plain_pen(old, cell)) {
                print_cell(out, cell);
            } else {
                write_cell(out, cell);
                pen = Some(cell.clone());
            }
            next_column = Some(
                column
                    + if cell.flags.contains(Flags::WIDE_CHAR) {
                        2
                    } else {
                        1
                    },
            );
            let next_is_spacer = column + 1 < grid.columns()
                && row[Column(column + 1)]
                    .flags
                    .contains(Flags::WIDE_CHAR_SPACER);
            if cell.flags.contains(Flags::WIDE_CHAR)
                && !next_is_spacer
                && column + 1 < grid.columns()
            {
                next_column = None;
                pen = None;
                // Rust can reproduce this with ECH, but xterm clears the whole
                // wide pair. Mark it non-exact for the actual display consumer.
                approximations.push("wide head with erased spacer cannot be represented by xterm");
                // Broken wide char (its spacer was erased): drop the new spacer.
                let _ = write!(out, "\x1b[{};{}H\x1b[X", line + 1, column + 2);
            }
        }
        let wrapped = row[Column(grid.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE);
        // The first print on the following row must trigger real autowrap:
        // CUP would turn that continuation into a hard line for copy/reflow.
        pending_soft_wrap = wrapped && next_column == Some(grid.columns());
        if wrapped && (!pending_soft_wrap || line + 1 == grid.screen_lines() as i32) {
            approximations
                .push("soft-wrap boundary cannot be reproduced without altering the grid");
        }
    }
}

// DCH/ICH can preserve combining data on a width-zero spacer. A synthetic wide
// head does not preserve that data: deleting the head discards its marks too.
fn write_spacer_marks(
    out: &mut Emitter,
    cell: &Cell,
    line: i32,
    column: usize,
    columns: usize,
    approximations: &mut Vec<&'static str>,
) {
    let Some(zero) = cell.zerowidth().filter(|zero| !zero.is_empty()) else {
        return;
    };
    if column == 0 {
        let _ = write!(out, "\x1b[{};1H", line + 1);
    } else if column + 2 < columns {
        // Alacritty backs up once more if the previous cell is a spacer. Put a
        // temporary orphan at c+1, then attach at c via a zero-width write from
        // c+2. The next columns are not drawn yet, so their scratch data is safe.
        let mut head = cell.clone();
        head.c = '中';
        head.flags.remove(Flags::WIDE_CHAR_SPACER);
        head.flags.insert(Flags::WIDE_CHAR);
        let _ = write!(out, "\x1b[{};{}H", line + 1, column + 2);
        write_cell(out, &head);
        let _ = write!(
            out,
            "\x1b[{};{}H\x1b[P\x1b[{};{}H",
            line + 1,
            column + 2,
            line + 1,
            column + 3
        );
    } else {
        approximations.push("combining data on a trailing wide-char spacer is not restored");
        return;
    }
    for &mark in zero {
        out.print(mark);
    }
    if column > 0 {
        // Writing over a spacer calls clear_wide on its left neighbor, which
        // would discard the just-restored marks. ECH clears only the temporary
        // cell before normal drawing reaches it.
        let _ = write!(out, "\x1b[{};{}H\x1b[X", line + 1, column + 2);
    }
}

fn write_stops(out: &mut Emitter, columns: impl Iterator<Item = usize>) {
    out.push_str("\x1b[3g");
    for column in columns {
        let _ = write!(out, "\x1b[1;{}H\x1bH", column + 1);
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
    sgr_color(out, cell.fg, true);
    sgr_color(out, cell.bg, false);
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

pub(crate) fn write_cell(out: &mut Emitter, cell: &Cell) {
    // Cursor restoration may leave an inherited hyperlink open. Cell rendering
    // must establish its own link rather than leak that pen into blank cells.
    out.push_str("\x1b]8;;\x1b\\");
    write_pen(out, cell);
    let link = cell.hyperlink();
    if let Some(link) = &link {
        let _ = write!(out, "\x1b]8;id={};{}\x1b\\", link.id(), link.uri());
    }
    print_cell(out, cell);
    if link.is_some() {
        out.push_str("\x1b]8;;\x1b\\");
    }
}

pub(crate) fn same_plain_pen(a: &Cell, b: &Cell) -> bool {
    let structural = Flags::WIDE_CHAR
        | Flags::WIDE_CHAR_SPACER
        | Flags::LEADING_WIDE_CHAR_SPACER
        | Flags::WRAPLINE;
    a.fg == b.fg
        && a.bg == b.bg
        && a.flags - structural == b.flags - structural
        && a.underline_color() == b.underline_color()
        && a.hyperlink().is_none()
        && b.hyperlink().is_none()
}
pub(crate) fn print_cell(out: &mut Emitter, cell: &Cell) {
    // A stored tab is a physical blank, not an HT command.
    out.print(if cell.c == '\t' { ' ' } else { cell.c });
    for &zero in cell.zerowidth().into_iter().flatten() {
        out.print(zero);
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
