//! Bounded read-only screen sources shared by live and naturally ended views.
use crate::TerminalProbe;
use crate::cold_history::ColdHistory;
use crate::history_page::{HistoryPage, HistoryPageError, HistoryRow, project_row};
use crate::history_search::{HistorySearch, HistorySearchRequest};
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Cell;

pub(crate) trait PrimaryText {
    fn history_rows(&self) -> usize;
    fn screen_rows(&self) -> usize;
    fn total_rows(&self) -> usize {
        self.history_rows() + self.screen_rows()
    }
    fn cold_rows(&self) -> usize;
    fn truncated(&self) -> bool;
    fn row(&self, index: usize) -> Result<HistoryRow, HistoryPageError>;
}
impl PrimaryText for TerminalProbe {
    fn history_rows(&self) -> usize {
        self.cold.len() + self.primary_text_grid().history_size()
    }
    fn screen_rows(&self) -> usize {
        self.primary_text_grid().screen_lines()
    }
    fn cold_rows(&self) -> usize {
        self.cold.len()
    }
    fn truncated(&self) -> bool {
        self.cold.truncated()
    }
    fn row(&self, index: usize) -> Result<HistoryRow, HistoryPageError> {
        if index < self.cold_rows() {
            return self.cold.row(index).ok_or(HistoryPageError::Range);
        }
        let grid = self.primary_text_grid();
        let row = index as i32 - self.cold_rows() as i32 - grid.history_size() as i32;
        project_row(&grid[Line(row)])
    }
}

/// Alternate-screen text never includes main-screen history or inactive screen contents.
pub(crate) enum ScreenText<'a> {
    Live(&'a Grid<Cell>),
    Frozen(&'a [HistoryRow]),
}
impl PrimaryText for ScreenText<'_> {
    fn history_rows(&self) -> usize {
        0
    }
    fn screen_rows(&self) -> usize {
        match self {
            Self::Live(grid) => grid.screen_lines(),
            Self::Frozen(rows) => rows.len(),
        }
    }
    fn cold_rows(&self) -> usize {
        0
    }
    fn truncated(&self) -> bool {
        false
    }
    fn row(&self, index: usize) -> Result<HistoryRow, HistoryPageError> {
        if index >= self.screen_rows() {
            return Err(HistoryPageError::Range);
        }
        match self {
            Self::Live(grid) => project_row(&grid[Line(index as i32)]),
            Self::Frozen(rows) => rows.get(index).cloned().ok_or(HistoryPageError::Range),
        }
    }
}
impl TerminalProbe {
    fn primary_text_grid(&self) -> &Grid<Cell> {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.term.inactive_grid()
        } else {
            self.term.grid()
        }
    }
    pub(crate) fn alternate_text(&self) -> Result<ScreenText<'_>, HistoryPageError> {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            return Err(HistoryPageError::Unavailable);
        }
        Ok(ScreenText::Live(self.term.grid()))
    }
}

/// Owns plain text only: no actor, driver, transport or native-resource lease.
pub(crate) struct FinalHistory {
    revision: u64,
    cold: ColdHistory,
    hot_and_screen: Vec<HistoryRow>,
    screen_rows: usize,
    alternate: Option<Vec<HistoryRow>>,
}
// Additional allocation budget shared by primary hot/screen and active alternate text.
// Compact cold rows transfer ownership without copying their existing allocation.
const FINAL_TEXT_BYTES: usize = 8 * 1024 * 1024;
fn freeze_rows(
    grid: &Grid<Cell>,
    history: usize,
    budget: usize,
    bytes: &mut usize,
) -> Result<Vec<HistoryRow>, HistoryPageError> {
    let count = history + grid.screen_lines();
    *bytes = bytes.saturating_add(count.saturating_mul(std::mem::size_of::<HistoryRow>()));
    if *bytes > budget {
        return Err(HistoryPageError::TooLarge);
    }
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        let row = project_row(&grid[Line(index as i32 - history as i32)])?;
        *bytes = bytes.saturating_add(row.text.capacity());
        if *bytes > budget {
            return Err(HistoryPageError::TooLarge);
        }
        rows.push(row);
    }
    Ok(rows)
}
impl FinalHistory {
    pub(crate) fn take(t: &mut TerminalProbe, revision: u64) -> Result<Self, HistoryPageError> {
        Self::take_with_budget(t, revision, FINAL_TEXT_BYTES)
    }
    fn take_with_budget(
        t: &mut TerminalProbe,
        revision: u64,
        budget: usize,
    ) -> Result<Self, HistoryPageError> {
        let grid = t.primary_text_grid();
        let screen_rows = grid.screen_lines();
        let mut bytes = 0;
        let rows = freeze_rows(grid, grid.history_size(), budget, &mut bytes)?;
        let alternate = if t.term.mode().contains(TermMode::ALT_SCREEN) {
            Some(freeze_rows(t.term.grid(), 0, budget, &mut bytes)?)
        } else {
            None
        };
        let cold = std::mem::replace(&mut t.cold, ColdHistory::new(0));
        Ok(Self {
            revision,
            cold,
            hot_and_screen: rows,
            screen_rows,
            alternate,
        })
    }
    pub(crate) fn page(
        &self,
        before: Option<usize>,
        revision: Option<u64>,
    ) -> Result<HistoryPage, HistoryPageError> {
        if revision.is_some_and(|r| r != self.revision) {
            return Err(HistoryPageError::Stale);
        }
        crate::history_page::primary_page(self, before, self.revision, false)
    }
    pub(crate) fn search(
        &self,
        request: &HistorySearchRequest,
    ) -> Result<HistorySearch, HistoryPageError> {
        if request.alternate {
            let rows = self
                .alternate
                .as_deref()
                .ok_or(HistoryPageError::Unavailable)?;
            crate::history_search::search_source(&ScreenText::Frozen(rows), request, self.revision)
        } else {
            crate::history_search::search_source(self, request, self.revision)
        }
    }
}
impl PrimaryText for FinalHistory {
    fn history_rows(&self) -> usize {
        self.cold_rows() + self.hot_and_screen.len() - self.screen_rows
    }
    fn screen_rows(&self) -> usize {
        self.screen_rows
    }
    fn cold_rows(&self) -> usize {
        self.cold.len()
    }
    fn truncated(&self) -> bool {
        self.cold.truncated()
    }
    fn row(&self, index: usize) -> Result<HistoryRow, HistoryPageError> {
        if index < self.cold_rows() {
            self.cold.row(index).ok_or(HistoryPageError::Range)
        } else {
            self.hot_and_screen
                .get(index - self.cold_rows())
                .cloned()
                .ok_or(HistoryPageError::Range)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_text_budget_refusal_keeps_source() -> Result<(), Box<dyn std::error::Error>> {
        let mut t = TerminalProbe::with_cold_history(pulse_core::TermSize::new(8, 2)?, 4);
        t.feed(b"first\r\nsecond\r\nthird\r\nfourth\r\nfifth\r\n");
        let before = t.history_page(None, 1)?;
        assert!(matches!(
            FinalHistory::take_with_budget(&mut t, 1, 0),
            Err(HistoryPageError::TooLarge)
        ));
        assert_eq!(t.history_page(None, 1)?.total_rows, before.total_rows);
        Ok(())
    }
    #[test]
    fn final_alternate_shares_budget_and_refusal_keeps_both_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut t = TerminalProbe::with_cold_history(pulse_core::TermSize::new(8, 2)?, 2);
        t.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n\x1b[?1049h\x1b[HALT");
        let before = t.history_page(None, 1)?;
        let mut primary_bytes = 0;
        freeze_rows(
            t.primary_text_grid(),
            t.primary_text_grid().history_size(),
            FINAL_TEXT_BYTES,
            &mut primary_bytes,
        )?;
        assert!(matches!(
            FinalHistory::take_with_budget(&mut t, 1, primary_bytes),
            Err(HistoryPageError::TooLarge)
        ));
        assert_eq!(t.history_page(None, 1)?.total_rows, before.total_rows);
        assert_eq!(t.alternate_text()?.row(0)?.text, "ALT");
        Ok(())
    }
}
