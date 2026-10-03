//! Replays a bounded suffix of PRIMARY history into a fresh renderer. History
//! lives in the backend; this is only a recent display cache, not stable line IDs
//! or the paginated deep-history/search API. Soft wraps are encoded naturally.
use crate::{
    RestoreGap, TerminalProbe,
    restore::{Emitter, print_cell, same_plain_pen, write_cell},
};
#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{
    TermMode,
    cell::{Cell, Flags},
};
/// Hard cap on history rows included in one diagnostic reconnect.
pub const MAX_RECENT_ROWS: usize = 2_000;
/// A recent-history suffix; no partial payload is returned on unsupported content.
#[derive(Debug)]
pub struct HistoryReplay {
    /// VT replay, ending in a blank viewport; feed before screen-only restore.
    pub bytes: Vec<u8>,
    /// Retained physical rows. Reflow may invalidate these coordinates.
    pub retained_rows: usize,
    /// Backend retained row count at the capture boundary.
    pub total_rows: usize,
}
/// Validation-only visual history projection; not a production pagination API.
#[cfg(any(test, feature = "validation"))]
#[derive(Debug, serde::Serialize)]
pub struct HistoryRowProjection {
    /// Visual character/colour/attribute cells at capture geometry.
    pub cells: Vec<crate::ObservedCell>,
    /// Whether this row continues the preceding retained row.
    pub wrapped: bool,
}
#[cfg(any(test, feature = "validation"))]
impl TerminalProbe {
    /// Project exactly the retained physical suffix selected by history replay.
    pub fn recent_history_projection(&self, retained: usize) -> Vec<HistoryRowProjection> {
        let grid = if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.term.inactive_grid()
        } else {
            self.term.grid()
        };
        let count = retained.min(MAX_RECENT_ROWS).min(grid.history_size());
        (-(count as i32)..0)
            .map(|line| HistoryRowProjection {
                cells: (0..grid.columns())
                    .map(|column| {
                        crate::observe(
                            &grid
                                [alacritty_terminal::index::Point::new(Line(line), Column(column))],
                        )
                    })
                    .collect(),
                wrapped: line > -(count as i32)
                    && grid[Line(line - 1)][Column(grid.columns() - 1)]
                        .flags
                        .contains(Flags::WRAPLINE),
            })
            .collect()
    }
}
impl TerminalProbe {
    /// Read primary history even while alternate screen is active. Feed into a
    /// freshly reset renderer of this size with enough scrollback for requested
    /// rows; then feed restore_screen_bounded and live bytes through ONE decoder.
    /// Cursor advances are artificial replay operations, not raw remote output.
    pub fn recent_history_bounded(
        &self,
        rows: usize,
        byte_budget: usize,
    ) -> Result<HistoryReplay, RestoreGap> {
        let grid = if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.term.inactive_grid()
        } else {
            self.term.grid()
        };
        let total = grid.history_size();
        let mut retained = rows.min(MAX_RECENT_ROWS).min(total);
        // Do not turn a truncated continuation into a new logical line. Round
        // the cutoff forward to the next hard-line boundary within this budget.
        while retained > 0
            && retained < total
            && grid[Line(-(retained as i32) - 1)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            retained -= 1;
        }
        if retained == 0 {
            return Ok(HistoryReplay {
                bytes: Vec::new(),
                retained_rows: 0,
                total_rows: total,
            });
        }
        let mut out = Emitter {
            buf: String::new(),
            last_print: None,
            limit: byte_budget,
            exceeded: false,
        };
        out.push_str("\x1b[0m\x1b(B\x0f\x1b]8;;\x1b\\");
        let mut pen: Option<Cell> = None;
        for line in -(retained as i32)..0 {
            let row = &grid[Line(line)];
            let wrap = row[Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE);
            let mut end = grid.columns();
            if !wrap {
                while end > 0 && row[Column(end - 1)] == Cell::default() {
                    end -= 1;
                }
            }
            for col in 0..end {
                let cell = &row[Column(col)];
                if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                    || (cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        && (col == 0 || !row[Column(col - 1)].flags.contains(Flags::WIDE_CHAR)))
                    || (cell.flags.contains(Flags::WIDE_CHAR)
                        && (col + 1 == grid.columns()
                            || !row[Column(col + 1)].flags.contains(Flags::WIDE_CHAR_SPACER)))
                {
                    return Err(RestoreGap::HistoryUnsupported);
                }
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                if pen.as_ref().is_some_and(|old| same_plain_pen(old, cell)) {
                    print_cell(&mut out, cell);
                } else {
                    write_cell(&mut out, cell);
                    pen = Some(cell.clone());
                }
                if out.exceeded {
                    return Err(RestoreGap::OutputTooLarge);
                }
            }
            if !wrap {
                // Erase skipped defaults with default pen, not the colour inherited
                // by the next blank row when the screen scrolls.
                out.push_str("\x1b[0m\x1b]8;;\x1b\\");
                pen = None;
                if end < grid.columns() {
                    out.push_str("\x1b[K");
                }
                out.push_str("\r\n");
            } else if line == -1 {
                // Force the final pending wrap so it remains a soft history row.
                out.push_str("\x1b[0m\x1b]8;;\x1b\\");
                out.print(' ');
            }
        }
        out.push_str("\x1b[0m\x1b]8;;\x1b\\");
        // Move all replay rows into scrollback; ED2 in restore clears only viewport.
        for _ in 1..grid.screen_lines() {
            out.push_str("\r\n");
        }
        if out.exceeded {
            return Err(RestoreGap::OutputTooLarge);
        }
        Ok(HistoryReplay {
            bytes: out.buf.into_bytes(),
            retained_rows: retained,
            total_rows: total,
        })
    }
}
