//! Mirrors the few terminal states that `alacritty_terminal` 0.26 keeps private
//! (scroll region, GL charset selection, tab stops) by observing exactly the same
//! `Handler` calls the `Term` receives. Every call is forwarded unchanged, so
//! parsing behaviour is identical; the mirror only records side state.
//!
//! The update rules are copied from alacritty_terminal 0.26.0 (`=0.26.0` pinned):
//! `set_scrolling_region`, DECCOLM, `resize`, `reset_state`, `set_active_charset`,
//! `set_horizontal_tabstop`, `clear_tabs`. Re-verify them on every upgrade; the
//! restore property tests fail if the mirror drifts.

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::vte::ansi::cursor_icon::CursorIcon;
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, NamedPrivateMode,
    PrivateMode, Rgb, ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
use std::collections::VecDeque;

/// Tab interval used by alacritty (`INITIAL_TABSTOPS`).
const INITIAL_TABSTOPS: usize = 8;

/// Terminal state that has no public getter in alacritty_terminal 0.26.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MirroredState {
    /// Scroll region as a half-open viewport line range `[top, bottom)`.
    pub scroll_region: (i32, i32),
    /// Charset currently invoked into GL (SO/SI).
    pub active_charset: CharsetIndex,
    /// One flag per column.
    pub tabs: Vec<bool>,
    /// Last printed character; vte keeps it privately for REP (`CSI b`) and never
    /// clears it (not even on RIS).
    pub last_input: Option<char>,
    /// Conservative known divergence hints; cleared only on verified display.
    pub risk: u32,
    /// Capture query-time overrides while synchronized output is flushed in bulk.
    pub color_queries: VecDeque<Option<Rgb>>,
}

impl MirroredState {
    pub fn new(columns: usize, lines: usize) -> Self {
        Self {
            scroll_region: (0, lines as i32),
            active_charset: CharsetIndex::G0,
            tabs: (0..columns)
                .map(|i| i.is_multiple_of(INITIAL_TABSTOPS))
                .collect(),
            last_input: None,
            risk: 0,
            color_queries: VecDeque::new(),
        }
    }

    /// Mirror of `Term::resize` (no-op when the dimensions are unchanged).
    pub fn on_resize(&mut self, old: (usize, usize), new: (usize, usize)) {
        let ((old_cols, old_lines), (cols, lines)) = (old, new);
        if old_cols == cols && old_lines == lines {
            return;
        }
        if old_cols != cols {
            let mut index = self.tabs.len();
            self.tabs.resize_with(cols, || {
                let stop = index.is_multiple_of(INITIAL_TABSTOPS);
                index += 1;
                stop
            });
        }
        self.scroll_region = (0, lines as i32);
        self.risk |= crate::reconciliation::GEOMETRY;
    }

    /// Mirror of `Term::set_scrolling_region` (also used by DECCOLM).
    fn apply_region(&mut self, lines: usize, top: usize, bottom: Option<usize>) {
        let bottom = bottom.unwrap_or(lines);
        if top >= bottom {
            return;
        }
        let lines = lines as i32;
        self.scroll_region = ((top as i32 - 1).min(lines), (bottom as i32).min(lines));
    }
}

/// Transparent `Handler` that forwards to `Term` and maintains [`MirroredState`].
pub(crate) struct Tracked<'a, L: EventListener> {
    pub term: &'a mut Term<L>,
    pub side: &'a mut MirroredState,
    pub history: &'a mut crate::cold_history::ColdHistory,
}

macro_rules! forward {
    ($(fn $name:ident(&mut self $(, $arg:ident : $ty:ty)*);)*) => {
        $(
            #[inline]
            fn $name(&mut self $(, $arg: $ty)*) {
                <Term<L> as Handler>::$name(&mut *self.term $(, $arg)*);
                self.history.collect(&mut *self.term);
            }
        )*
    };
}

impl<L: EventListener> Handler for Tracked<'_, L> {
    forward! {
        fn set_title(&mut self, title: Option<String>);
        fn set_cursor_style(&mut self, style: Option<CursorStyle>);
        fn set_cursor_shape(&mut self, shape: CursorShape);
        fn goto(&mut self, line: i32, col: usize);
        fn goto_line(&mut self, line: i32);
        fn goto_col(&mut self, col: usize);

        fn move_up(&mut self, rows: usize);
        fn move_down(&mut self, rows: usize);
        fn identify_terminal(&mut self, intermediate: Option<char>);
        fn device_status(&mut self, arg: usize);
        fn move_forward(&mut self, cols: usize);
        fn move_backward(&mut self, cols: usize);
        fn move_down_and_cr(&mut self, rows: usize);
        fn move_up_and_cr(&mut self, rows: usize);

        fn backspace(&mut self);
        fn carriage_return(&mut self);
        fn linefeed(&mut self);
        fn bell(&mut self);
        fn substitute(&mut self);
        fn newline(&mut self);
        fn scroll_up(&mut self, rows: usize);
        fn scroll_down(&mut self, rows: usize);






        fn save_cursor_position(&mut self);



        fn set_tabs(&mut self, interval: u16);
        fn reverse_index(&mut self);
        fn terminal_attribute(&mut self, attr: Attr);
        fn set_mode(&mut self, mode: Mode);
        fn unset_mode(&mut self, mode: Mode);
        fn report_mode(&mut self, mode: Mode);
        fn report_private_mode(&mut self, mode: PrivateMode);
        fn set_keypad_application_mode(&mut self);
        fn unset_keypad_application_mode(&mut self);
        fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset);
        fn set_color(&mut self, index: usize, color: Rgb);

        fn reset_color(&mut self, index: usize);
        fn clipboard_store(&mut self, clipboard: u8, data: &[u8]);
        fn clipboard_load(&mut self, clipboard: u8, terminator: &str);
        fn decaln(&mut self);
        fn push_title(&mut self);
        fn pop_title(&mut self);
        fn text_area_size_pixels(&mut self);
        fn text_area_size_chars(&mut self);
        fn set_hyperlink(&mut self, link: Option<Hyperlink>);
        fn set_mouse_cursor_icon(&mut self, icon: CursorIcon);
        fn report_keyboard_mode(&mut self);
        fn push_keyboard_mode(&mut self, mode: KeyboardModes);
        fn pop_keyboard_modes(&mut self, count: u16);
        fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys);
        fn report_modify_other_keys(&mut self);
        fn set_scp(&mut self, path: ScpCharPath, update: ScpUpdateMode);
    }

    fn put_tab(&mut self, count: u16) {
        self.side.risk |= crate::reconciliation::TAB;
        <Term<L> as Handler>::put_tab(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn move_backward_tabs(&mut self, count: u16) {
        self.side.risk |= crate::reconciliation::TAB;
        <Term<L> as Handler>::move_backward_tabs(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn move_forward_tabs(&mut self, count: u16) {
        self.side.risk |= crate::reconciliation::TAB;
        <Term<L> as Handler>::move_forward_tabs(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn insert_blank_lines(&mut self, count: usize) {
        self.side.risk |= crate::reconciliation::LINES;
        <Term<L> as Handler>::insert_blank_lines(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn delete_lines(&mut self, count: usize) {
        self.side.risk |= crate::reconciliation::LINES;
        <Term<L> as Handler>::delete_lines(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn insert_blank(&mut self, count: usize) {
        self.side.risk |= crate::reconciliation::CELLS;
        <Term<L> as Handler>::insert_blank(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn delete_chars(&mut self, count: usize) {
        self.side.risk |= crate::reconciliation::CELLS;
        <Term<L> as Handler>::delete_chars(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn erase_chars(&mut self, count: usize) {
        self.side.risk |= crate::reconciliation::CELLS;
        <Term<L> as Handler>::erase_chars(&mut *self.term, count);
        self.history.collect(&mut *self.term);
    }
    fn clear_line(&mut self, mode: LineClearMode) {
        self.side.risk |= crate::reconciliation::CELLS;
        <Term<L> as Handler>::clear_line(&mut *self.term, mode);
        self.history.collect(&mut *self.term);
    }
    fn clear_screen(&mut self, mode: ClearMode) {
        self.side.risk |= crate::reconciliation::CELLS;
        if matches!(mode, ClearMode::Saved)
            && !self
                .term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
        {
            self.history.clear();
        }
        <Term<L> as Handler>::clear_screen(&mut *self.term, mode);
        self.history.collect(&mut *self.term);
    }
    fn restore_cursor_position(&mut self) {
        self.side.risk |= crate::reconciliation::CURSOR;
        <Term<L> as Handler>::restore_cursor_position(&mut *self.term);
        self.history.collect(&mut *self.term);
    }
    fn input(&mut self, c: char) {
        <Term<L> as Handler>::input(&mut *self.term, c);
        self.history.collect(&mut *self.term);
        self.side.last_input = Some(c);
    }

    fn set_horizontal_tabstop(&mut self) {
        let column = self.term.grid().cursor.point.column.0;
        <Term<L> as Handler>::set_horizontal_tabstop(&mut *self.term);
        self.history.collect(&mut *self.term);
        if let Some(stop) = self.side.tabs.get_mut(column) {
            *stop = true;
        }
    }

    fn clear_tabs(&mut self, mode: TabulationClearMode) {
        let column = self.term.grid().cursor.point.column.0;
        let all = matches!(mode, TabulationClearMode::All);
        <Term<L> as Handler>::clear_tabs(&mut *self.term, mode);
        self.history.collect(&mut *self.term);
        match all {
            false => {
                if let Some(stop) = self.side.tabs.get_mut(column) {
                    *stop = false;
                }
            }
            true => self.side.tabs.fill(false),
        }
    }

    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        if index < alacritty_terminal::term::color::COUNT {
            self.side.color_queries.push_back(self.term.colors()[index]);
        }
        <Term<L> as Handler>::dynamic_color_sequence(&mut *self.term, prefix, index, terminator);
        self.history.collect(&mut *self.term);
    }

    fn reset_state(&mut self) {
        self.history.clear();
        <Term<L> as Handler>::reset_state(&mut *self.term);
        self.history.collect(&mut *self.term);
        let risk = self.side.risk;
        let last_input = self.side.last_input;
        let queries = std::mem::take(&mut self.side.color_queries);
        *self.side = MirroredState::new(self.term.columns(), self.term.screen_lines());
        self.side.last_input = last_input;
        self.side.risk = risk;
        self.side.color_queries = queries;
    }

    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        let lines = self.term.screen_lines();
        <Term<L> as Handler>::set_scrolling_region(&mut *self.term, top, bottom);
        self.history.collect(&mut *self.term);
        self.side.apply_region(lines, top, bottom);
    }

    fn set_active_charset(&mut self, index: CharsetIndex) {
        <Term<L> as Handler>::set_active_charset(&mut *self.term, index);
        self.history.collect(&mut *self.term);
        self.side.active_charset = index;
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        self.side.risk |= crate::reconciliation::MODE;
        let deccolm = matches!(mode, PrivateMode::Named(NamedPrivateMode::ColumnMode));
        let lines = self.term.screen_lines();
        <Term<L> as Handler>::set_private_mode(&mut *self.term, mode);
        self.history.collect(&mut *self.term);
        if deccolm {
            self.side.apply_region(lines, 1, None);
        }
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        self.side.risk |= crate::reconciliation::MODE;
        let deccolm = matches!(mode, PrivateMode::Named(NamedPrivateMode::ColumnMode));
        let lines = self.term.screen_lines();
        <Term<L> as Handler>::unset_private_mode(&mut *self.term, mode);
        self.history.collect(&mut *self.term);
        if deccolm {
            self.side.apply_region(lines, 1, None);
        }
    }
}
