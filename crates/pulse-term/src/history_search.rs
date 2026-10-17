//! Bounded literal and regular-expression searches over retained terminal text.
use crate::{HistoryPage, HistoryPageError, TerminalProbe};

#[path = "history_search_regex.rs"]
mod regex_search;
#[path = "history_search_window.rs"]
mod window;

/// Maximum physical rows scanned on one actor turn, including overlap.
pub const SEARCH_ROWS: usize = 2048;
/// Maximum UTF-8 source text scanned on one actor turn.
pub const SEARCH_BYTES: usize = 512 * 1024;
/// Maximum UTF-8 bytes in a literal search query.
pub const QUERY_BYTES: usize = 512;

/// A search cursor is a gap between physical rows, bound to the exact actor revision.
#[derive(Clone)]
pub struct HistorySearchRequest {
    /// Literal Unicode text or an expression; empty or control-bearing text is rejected.
    pub query: String,
    /// Enable case-sensitive matching; text is not Unicode-normalized.
    pub match_case: bool,
    /// Use a bounded non-backtracking expression, one match per logical line.
    pub regex: bool,
    /// Require Unicode word boundaries around the entire literal or expression.
    pub whole_word: bool,
    /// Search older rows when true, newer rows otherwise.
    pub backwards: bool,
    /// Search only the active alternate screen, without main-screen history.
    pub alternate: bool,
    /// Gap to continue from; absent starts at the appropriate retained edge.
    pub cursor: Option<usize>,
    /// Revision required for every continuation.
    pub revision: Option<u64>,
}

/// One bounded scan; contextual pages include primary screen rows only for search.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySearch {
    /// Decimal actor revision, never a JavaScript number.
    pub revision: String,
    /// Count of retained history plus primary screen rows.
    pub total_rows: usize,
    /// Primary screen rows included in total_rows, at most 256.
    pub screen_rows: usize,
    /// Explicit source scope; alternate pages never contain main-screen history.
    pub alternate: bool,
    /// Echo the requested matching mode.
    pub regex: bool,
    /// Echo whether the match requires Unicode word boundaries.
    pub whole_word: bool,
    /// Next gap; unmatched soft-wrap suffixes/prefixes are reexamined next turn.
    pub cursor: usize,
    /// The requested retained edge has been reached.
    pub complete: bool,
    /// First physical row touched by a match.
    pub matched_row: Option<usize>,
    /// Exclusive end of the matching physical row span.
    pub matched_end: Option<usize>,
    /// Bounded page ending at matched_end; never a history navigation cursor.
    pub page: Option<HistoryPage>,
}

impl TerminalProbe {
    /// Search without changing parser or input state, following only actual soft wraps.
    pub fn history_search(
        &self,
        request: &HistorySearchRequest,
        revision: u64,
    ) -> Result<HistorySearch, HistoryPageError> {
        if request.alternate {
            search_source(&self.alternate_text()?, request, revision)
        } else {
            search_source(self, request, revision)
        }
    }
}
pub(crate) fn search_source(
    source: &impl crate::primary_text::PrimaryText,
    request: &HistorySearchRequest,
    revision: u64,
) -> Result<HistorySearch, HistoryPageError> {
    if request.query.is_empty()
        || request.query.len() > QUERY_BYTES
        || request.query.chars().any(char::is_control)
        || request.cursor.is_some() != request.revision.is_some()
    {
        return Err(HistoryPageError::Range);
    }
    if request.revision.is_some_and(|value| value != revision) {
        return Err(HistoryPageError::Stale);
    }
    let total = source.total_rows();
    let start = request
        .cursor
        .unwrap_or(if request.backwards { total } else { 0 });
    if start > total {
        return Err(HistoryPageError::Range);
    }
    if request.regex || request.whole_word {
        let (cursor, matched) = regex_search::scan(source, request, start)?;
        return response(source, request, revision, cursor, matched);
    }
    let query = if request.match_case {
        request.query.clone()
    } else {
        request.query.to_lowercase()
    };
    let mut cursor = start;
    let mut bytes = 0;
    let mut matched = None;
    let mut window = window::Window::default();
    for _ in 0..SEARCH_ROWS {
        let index = if request.backwards {
            let Some(index) = cursor.checked_sub(1) else {
                break;
            };
            index
        } else {
            if cursor == total {
                break;
            }
            cursor
        };
        let mut row = source.row(index)?;
        if bytes + row.text.len() > SEARCH_BYTES {
            break;
        }
        bytes += row.text.len();
        cursor = if request.backwards { index } else { index + 1 };
        if !request.match_case {
            row.text = row.text.to_lowercase();
        }
        if let Some(span) = window.push(index, row, request.backwards, &query) {
            cursor = if request.backwards {
                span.start
            } else {
                span.end
            };
            matched = Some(span);
            break;
        }
    }
    let complete = cursor == if request.backwards { 0 } else { total };
    if matched.is_none() && !complete {
        cursor = window.cursor(request.backwards).unwrap_or(cursor);
        if cursor == start {
            return Err(HistoryPageError::TooLarge);
        }
    }
    response(source, request, revision, cursor, matched)
}

fn response(
    source: &impl crate::primary_text::PrimaryText,
    request: &HistorySearchRequest,
    revision: u64,
    cursor: usize,
    matched: Option<std::ops::Range<usize>>,
) -> Result<HistorySearch, HistoryPageError> {
    let total = source.total_rows();
    let screen_rows = source.screen_rows();
    let complete = cursor == if request.backwards { 0 } else { total };
    let page = matched
        .as_ref()
        .map(|span| crate::history_page::primary_page(source, Some(span.end), revision, true))
        .transpose()?;
    Ok(HistorySearch {
        revision: revision.to_string(),
        total_rows: total,
        screen_rows,
        alternate: request.alternate,
        regex: request.regex,
        whole_word: request.whole_word,
        cursor,
        complete,
        matched_row: matched.as_ref().map(|span| span.start),
        matched_end: matched.as_ref().map(|span| span.end),
        page,
    })
}

#[cfg(test)]
#[path = "history_search_tests.rs"]
mod tests;
