//! Read-only bounded physical history pages; never replayed into the live view.
use crate::TerminalProbe;
use alacritty_terminal::grid::Row;
use alacritty_terminal::index::Column;
use alacritty_terminal::term::cell::{Cell, Flags};

/// Maximum physical rows returned by one read.
pub const PAGE_ROWS: usize = 200;
/// Maximum UTF-8 text bytes returned by one read.
pub const PAGE_BYTES: usize = 128 * 1024;

/// A physical history row; remote text is untrusted.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    /// Cell text without VT instructions, preserving combining marks.
    pub text: String,
    /// This row continues into the following physical row.
    pub wrapped: bool,
}
/// One revision-protected page; end is exclusive, oldest retained row is zero.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    /// Opaque actor revision, encoded as decimal to avoid JavaScript precision loss.
    pub revision: String,
    /// Source rows: history reads exclude the screen; search pages include primary screen rows.
    pub total_rows: usize,
    /// Rows held in compact storage outside the hot restore grid.
    pub cold_rows: usize,
    /// Earlier content was discarded by a retention or projection budget.
    pub truncated: bool,
    /// First returned row, relative to oldest retained history.
    pub start: usize,
    /// Exclusive end, and cursor for a subsequent newer read.
    pub end: usize,
    /// Untrusted plain text, not HTML or terminal instructions.
    pub rows: Vec<HistoryRow>,
}
/// Typed history read failures; none authorize retry or input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HistoryPageError {
    /// Invalid or over-budget regular expression.
    #[error("invalid or over-budget search expression")]
    Pattern,
    /// The requested range is outside currently retained history.
    #[error("history cursor is outside retained rows")]
    Range,
    /// A single row exceeds the per-page byte budget.
    #[error("history row exceeds byte budget")]
    TooLarge,
    /// Output or resize changed the source since the caller's page.
    #[error("history source changed")]
    Stale,
    /// Actor cannot accept a read now.
    #[error("history actor is unavailable")]
    Unavailable,
}

impl From<crate::session::SessionError> for HistoryPageError {
    fn from(_: crate::session::SessionError) -> Self {
        Self::Unavailable
    }
}

pub(crate) fn project_row(row: &Row<Cell>) -> Result<HistoryRow, HistoryPageError> {
    let columns = row.len();
    let wrapped = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
    // Remove only trailing default cells. Preserve explicit attributes, wrapping,
    // wide-cell spacers and combining sequences, rather than trim Unicode whitespace.
    let end = if wrapped {
        columns
    } else {
        (0..columns)
            .rfind(|i| row[Column(*i)] != Cell::default())
            .map_or(0, |i| i + 1)
    };
    let mut text = String::new();
    for column in 0..end {
        let cell = &row[Column(column)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let c = if cell.c == '\t' { ' ' } else { cell.c };
        let combining_bytes = cell
            .zerowidth()
            .map_or(0, |chars| chars.iter().map(|c| c.len_utf8()).sum());
        if text
            .len()
            .saturating_add(c.len_utf8())
            .saturating_add(combining_bytes)
            > PAGE_BYTES
        {
            return Err(HistoryPageError::TooLarge);
        }
        text.push(c);
        if let Some(combining) = cell.zerowidth() {
            text.extend(combining);
        }
    }
    Ok(HistoryRow { text, wrapped })
}

impl TerminalProbe {
    /// Read main-screen hot and cold history, including while an alternate screen is active.
    /// The caller supplies the actor revision captured at the same serial boundary.
    pub fn history_page(
        &self,
        before: Option<usize>,
        revision: u64,
    ) -> Result<HistoryPage, HistoryPageError> {
        self.primary_page(before, revision, false)
    }

    /// A separate search projection can include the primary screen without changing history paging.
    pub(crate) fn primary_page(
        &self,
        before: Option<usize>,
        revision: u64,
        include_screen: bool,
    ) -> Result<HistoryPage, HistoryPageError> {
        primary_page(self, before, revision, include_screen)
    }
}

pub(crate) fn primary_page(
    source: &impl crate::primary_text::PrimaryText,
    before: Option<usize>,
    revision: u64,
    include_screen: bool,
) -> Result<HistoryPage, HistoryPageError> {
    let cold_rows = source.cold_rows();
    let total = source.total_rows()
        - if include_screen {
            0
        } else {
            source.screen_rows()
        };
    let end = before.unwrap_or(total);
    if end > total {
        return Err(HistoryPageError::Range);
    }
    let mut start = end;
    let mut bytes = 0usize;
    let mut rows = Vec::new();
    while start > end.saturating_sub(PAGE_ROWS) {
        let row = source.row(start - 1)?;
        if bytes + row.text.len() > PAGE_BYTES {
            break;
        }
        bytes += row.text.len();
        rows.push(row);
        start -= 1;
    }
    rows.reverse();
    Ok(HistoryPage {
        revision: revision.to_string(),
        total_rows: total,
        cold_rows,
        truncated: source.truncated(),
        start,
        end,
        rows,
    })
}
