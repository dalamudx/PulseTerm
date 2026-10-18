//! Compact, bounded primary history beyond the renderer's hot cell grid.
use std::collections::VecDeque;
use std::mem::size_of;

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::{Config, TermMode};

use crate::history_page::{HistoryRow, project_row};

#[cfg(test)]
#[path = "cold_history_tests.rs"]
mod tests;

pub(crate) const MAX_ROWS: usize = 100_000;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const BLOCK_ROWS: usize = 256;
const BLOCK_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct Block {
    // Each offset starts a one-byte wrap flag followed by valid UTF-8 text.
    data: String,
    offsets: Vec<u32>,
    first: usize,
    // Private monotonic row coordinate, independent of front-block trimming.
    start: usize,
}
impl Block {
    fn bytes(&self) -> usize {
        self.data.capacity() + self.offsets.capacity() * size_of::<u32>()
    }
    fn row(&self, index: usize) -> HistoryRow {
        let index = self.first + index;
        let start = self.offsets[index] as usize;
        let end = self
            .offsets
            .get(index + 1)
            .map_or(self.data.len(), |n| *n as usize);
        HistoryRow {
            text: self.data[start + 1..end].to_owned(),
            wrapped: self.data.as_bytes()[start] != 0,
        }
    }
}

pub(crate) struct ColdHistory {
    enabled: bool,
    hot: usize,
    limit: usize,
    blocks: VecDeque<Block>,
    rows: usize,
    next_row: usize,
    bytes: usize,
    byte_limit: usize,
    truncated: bool,
    // Matches TerminalProbe's options; only scrolling_history is varied.
    config: Config,
}
impl ColdHistory {
    pub(crate) fn new(hot: usize) -> Self {
        Self {
            enabled: false,
            hot,
            limit: MAX_ROWS,
            blocks: VecDeque::new(),
            rows: 0,
            next_row: 0,
            bytes: 0,
            byte_limit: MAX_BYTES,
            truncated: false,
            config: Config {
                scrolling_history: MAX_ROWS,
                osc52: alacritty_terminal::term::Osc52::Disabled,
                ..Config::default()
            },
        }
    }
    pub(crate) fn enable<L: EventListener>(&mut self, term: &mut Term<L>) {
        // A zero-history session continues to retain no history.
        if self.hot != 0 {
            self.enabled = true;
            term.set_options(self.config.clone());
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.rows
    }
    pub(crate) fn truncated(&self) -> bool {
        self.truncated
    }
    fn memory_bytes(&self) -> usize {
        self.bytes + self.blocks.capacity() * size_of::<Block>()
    }
    fn discard_oldest(&mut self) {
        if let Some(block) = self.blocks.front_mut() {
            block.first += 1;
            self.rows -= 1;
            self.truncated = true;
            if block.first == block.offsets.len() {
                self.bytes -= block.bytes();
                self.blocks.pop_front();
            }
        }
    }
    pub(crate) fn set_limit(&mut self, limit: usize) {
        self.limit = limit.min(MAX_ROWS);
        self.hot = self.hot.min(self.limit);
    }
    fn trim(&mut self, hot_rows: usize) {
        while self.rows > self.limit.saturating_sub(hot_rows)
            || self.memory_bytes() > self.byte_limit
        {
            if self.rows == 0 {
                self.blocks.shrink_to_fit();
                break;
            }
            self.discard_oldest();
        }
    }
    fn push(&mut self, row: HistoryRow) {
        // Rebase before the private ordinal can overflow in a long-lived session.
        if self.next_row == usize::MAX {
            let base = self
                .blocks
                .front()
                .map_or(self.next_row, |block| block.start);
            for block in &mut self.blocks {
                block.start -= base;
            }
            self.next_row -= base;
        }
        let new_block = self.blocks.back().is_none_or(|block| {
            block.offsets.len() >= BLOCK_ROWS || block.data.len() + row.text.len() + 1 > BLOCK_BYTES
        });
        if new_block {
            // A sealed block will never grow again. Release growth slack once,
            // while leaving the active block amortized and row offsets unchanged.
            if let Some(block) = self.blocks.back_mut() {
                let before = block.bytes();
                block.data.shrink_to_fit();
                block.offsets.shrink_to_fit();
                self.bytes -= before - block.bytes();
            }
            self.blocks.push_back(Block {
                start: self.next_row,
                ..Block::default()
            });
        }
        if let Some(block) = self.blocks.back_mut() {
            let before = block.bytes();
            block.offsets.push(block.data.len() as u32);
            block.data.push(if row.wrapped { '\u{1}' } else { '\0' });
            block.data.push_str(&row.text);
            self.bytes += block.bytes() - before;
            self.rows += 1;
            self.next_row += 1;
        }
        self.trim(0);
    }
    pub(crate) fn row(&self, index: usize) -> Option<HistoryRow> {
        if index >= self.rows {
            return None;
        }
        let absolute = self.next_row - self.rows + index;
        let block_index = self.blocks.partition_point(|block| block.start <= absolute) - 1;
        let block = &self.blocks[block_index];
        Some(block.row(absolute - block.start - block.first))
    }
    pub(crate) fn clear(&mut self) {
        self.blocks = VecDeque::new();
        self.rows = 0;
        self.next_row = 0;
        self.bytes = 0;
        self.truncated = false;
    }
    /// Called after actual SDK Handler operations and resize, never by byte-line guessing.
    pub(crate) fn collect<L: EventListener>(&mut self, term: &mut Term<L>) {
        if !self.enabled {
            return;
        }
        let alternate = term.mode().contains(TermMode::ALT_SCREEN);
        let grid = if alternate {
            term.inactive_grid()
        } else {
            term.grid()
        };
        let total = grid.history_size();
        let excess = total.saturating_sub(self.hot);
        for offset in 0..excess {
            let row = &grid[Line(-(total as i32) + offset as i32)];
            match project_row(row) {
                Ok(row) => self.push(row),
                Err(_) => {
                    // Keep a contiguous suffix: an unrepresentable row discards
                    // that row and everything older, rather than hiding a gap.
                    self.clear();
                    self.truncated = true;
                }
            }
        }
        if excess != 0 {
            if alternate {
                // Existing public set_options targets the inactive PRIMARY grid.
                // Other options are unchanged; title events are ignored by ReplyListener.
                let mut options = self.config.clone();
                options.scrolling_history = self.hot;
                term.set_options(options);
                term.set_options(self.config.clone());
            } else {
                let grid = term.grid_mut();
                grid.update_history(self.hot);
                grid.update_history(MAX_ROWS);
            }
        }
        self.trim(total.min(self.hot));
    }
}
