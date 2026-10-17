//! A query-sized overlap window; row spans are byte coordinates in folded text.
use crate::HistoryRow;
use std::ops::Range;

#[derive(Default)]
pub(super) struct Window {
    text: String,
    spans: Vec<(usize, usize)>,
    wrapped: bool,
}
impl Window {
    pub(super) fn push(
        &mut self,
        index: usize,
        row: HistoryRow,
        backwards: bool,
        query: &str,
    ) -> Option<Range<usize>> {
        if if backwards {
            !row.wrapped
        } else {
            !self.wrapped
        } {
            self.text.clear();
            self.spans.clear();
        }
        self.wrapped = row.wrapped;
        if backwards {
            let offset = row.text.len();
            let mut text = row.text;
            text.push_str(&self.text);
            self.text = text;
            for (end, _) in &mut self.spans {
                *end += offset;
            }
            if offset > 0 {
                self.spans.insert(0, (offset, index));
            }
        } else {
            self.text.push_str(&row.text);
            if !row.text.is_empty() {
                self.spans.push((self.text.len(), index));
            }
        }
        let hit = if backwards {
            self.text.rfind(query)
        } else {
            self.text.find(query)
        };
        if let Some(at) = hit {
            let start = self.spans.iter().find(|(end, _)| *end > at)?.1;
            let end = self
                .spans
                .iter()
                .find(|(end, _)| *end >= at + query.len())?
                .1
                + 1;
            return Some(start..end);
        }
        let keep = query.len() - 1;
        if backwards {
            let mut end = self.text.len().min(keep);
            while !self.text.is_char_boundary(end) {
                end -= 1;
            }
            self.text.truncate(end);
            self.spans.retain(|(offset, _)| *offset > 0);
            let mut before = 0;
            self.spans.retain_mut(|(offset, _)| {
                let retain = before < end;
                before = *offset;
                *offset = (*offset).min(end);
                retain
            });
        } else {
            let mut start = self.text.len().saturating_sub(keep);
            while !self.text.is_char_boundary(start) {
                start += 1;
            }
            self.text.drain(..start);
            self.spans.retain_mut(|(end, _)| {
                *end = end.saturating_sub(start);
                *end > 0
            });
            if !self.wrapped {
                self.text.clear();
                self.spans.clear();
            }
        }
        None
    }
    pub(super) fn cursor(&self, backwards: bool) -> Option<usize> {
        if backwards {
            self.spans.last().map(|(_, row)| row + 1)
        } else {
            self.spans.first().map(|(_, row)| *row)
        }
    }
}
