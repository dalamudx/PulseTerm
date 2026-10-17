//! Complete logical-line matching with bounded compilation and input.
use super::{HistorySearchRequest, SEARCH_BYTES, SEARCH_ROWS};
use crate::{HistoryPageError, primary_text::PrimaryText};
use std::ops::Range;

type Scan = (usize, Option<Range<usize>>);
pub(super) fn scan(
    source: &impl PrimaryText,
    request: &HistorySearchRequest,
    start: usize,
) -> Result<Scan, HistoryPageError> {
    let pattern = if request.regex {
        request.query.clone()
    } else {
        regex::escape(&request.query)
    };
    let pattern = if request.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    let expression = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!request.match_case)
        .size_limit(1024 * 1024)
        .dfa_size_limit(1024 * 1024)
        .nest_limit(64)
        .build()
        .map_err(|_| HistoryPageError::Pattern)?;
    let total = source.total_rows();
    if start > 0 && start < total && source.row(start - 1)?.wrapped {
        return Err(HistoryPageError::Range);
    }
    let mut cursor = start;
    let mut bytes = 0;
    let mut parts = Vec::new();
    let mut span = start..start;
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
        let row = source.row(index)?;
        if bytes + row.text.len() > SEARCH_BYTES {
            break;
        }
        bytes += row.text.len();
        // A non-wrapped predecessor closes the later line before it is consumed.
        if request.backwards && !row.wrapped && !parts.is_empty() {
            if matches(&expression, &parts, true) {
                return Ok((span.start, Some(span)));
            }
            parts.clear();
        }
        if parts.is_empty() {
            span = index..index + 1;
        } else {
            span.start = span.start.min(index);
            span.end = span.end.max(index + 1);
        }
        parts.push(row.text);
        cursor = if request.backwards { index } else { index + 1 };
        if (!request.backwards && (!row.wrapped || cursor == total))
            || (request.backwards && cursor == 0)
        {
            if matches(&expression, &parts, request.backwards) {
                return Ok((
                    if request.backwards {
                        span.start
                    } else {
                        span.end
                    },
                    Some(span),
                ));
            }
            parts.clear();
        }
    }
    // Revisit an unfinished logical line next turn; never test partial anchors.
    if !parts.is_empty() {
        cursor = if request.backwards {
            span.end
        } else {
            span.start
        };
    }
    if cursor == start && cursor != if request.backwards { 0 } else { total } {
        return Err(HistoryPageError::TooLarge);
    }
    Ok((cursor, None))
}
fn matches(expression: &regex::Regex, parts: &[String], backwards: bool) -> bool {
    let mut text = String::with_capacity(parts.iter().map(String::len).sum());
    if backwards {
        for part in parts.iter().rev() {
            text.push_str(part);
        }
    } else {
        for part in parts {
            text.push_str(part);
        }
    }
    expression.is_match(&text)
}

#[cfg(test)]
#[path = "history_search_regex_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "history_search_whole_word_tests.rs"]
mod whole_word_tests;
