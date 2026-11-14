// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! The editing commands of a field: caret moves by character, word, row and
//! paragraph, deletes, the kill buffer, Undo and Redo, and the clipboard.
use std::ops::Range;

use super::editing::{self, Snapshot, Typing};
use super::element::{hit_row, row_index, rows_of, x_in};
use super::*;
use gpui::{ClipboardItem, KeyDownEvent};
thread_local! {
    /// What Control-K last cut, which Control-Y puts back: one buffer for
    /// every field, as the Mac keeps one per app.
    static KILL_BUFFER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

impl TextInput {
    /// The text and selection, as the history keeps them.
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.content.to_string(),
            selection: self.selected_range.clone(),
        }
    }

    /// Where the selection is held from: the end the caret is not at.
    pub(super) fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    /// An offset no further than the text and on a character's edge, for
    /// positions read from a layout drawn before the last edit.
    pub(super) fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.content.len());
        while !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// The caret moved or the selection changed by itself: typing that
    /// follows starts a new step to undo, and a kill is not added to.
    pub(super) fn moved(&mut self) {
        self.history.close();
        self.killing = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
    }

    /// Replaces `range` with `text`, as one step to undo, or as part of the
    /// run of typing it continues. The caret goes after the new text.
    pub(super) fn edit(
        &mut self,
        range: Range<usize>,
        text: &str,
        typing: Option<Typing>,
        cx: &mut Context<Self>,
    ) {
        let range = self.clamp(range.start)..self.clamp(range.end);
        if range.is_empty() && text.is_empty() {
            return;
        }
        self.history.record(self.snapshot(), typing);
        self.splice(range, text, cx);
    }

    /// Replaces `range` with `text`, outside the history.
    pub(super) fn splice(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.content =
            (self.content[0..range.start].to_owned() + text + &self.content[range.end..]).into();
        let caret = range.start + text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        self.killing = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
        cx.notify();
    }

    /// Takes the text back to a state from the history. What Undo brings
    /// back is selected from its end, as Chromium leaves it.
    fn restore(&mut self, state: Snapshot, reversed: bool, cx: &mut Context<Self>) {
        self.content = state.text.into();
        self.selected_range = self.clamp(state.selection.start)..self.clamp(state.selection.end);
        self.selection_reversed = reversed && !self.selected_range.is_empty();
        self.marked_range = None;
        self.moved();
        cx.notify();
    }

    /// Deletes the selection if there is one, or else `range`.
    fn delete_or_selection(
        &mut self,
        range: Range<usize>,
        typing: Option<Typing>,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            range
        } else {
            self.selected_range.clone()
        };
        self.edit(range, "", typing, cx);
    }

    /// Moves the caret to `offset`, or with `extend` moves the selection's
    /// free end there. `upstream` draws a caret at a wrap after the upper
    /// line rather than before the lower.
    pub(super) fn go(
        &mut self,
        offset: usize,
        upstream: bool,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let offset = self.clamp(offset);
        if extend {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        self.upstream = upstream;
    }

    pub(super) fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    pub(super) fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    pub(super) fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(super) fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    pub(super) fn select_to_start(
        &mut self,
        _: &SelectToStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    pub(super) fn select_to_end(
        &mut self,
        _: &SelectToEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.content.len(), cx);
    }

    /// Option-Left (Control-Left elsewhere) moves from the caret's end of a
    /// selection, as Chromium does, rather than from its start.
    pub(super) fn word_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        let target = editing::word_start_before(&self.content, self.cursor_offset());
        self.go(target, false, extend, cx);
    }

    pub(super) fn word_right(&mut self, extend: bool, cx: &mut Context<Self>) {
        let target = editing::word_right(&self.content, self.cursor_offset());
        self.go(target, false, extend, cx);
    }

    /// The row the caret is on as drawn: its range, and whether its end is
    /// a wrap rather than the end of a line.
    fn row_at(&self, offset: usize) -> (Range<usize>, bool) {
        match self.last_lines.as_ref() {
            Some(lines) => {
                let rows = rows_of(lines, px(self.kind.line_height()));
                let row = &rows[row_index(&rows, offset, self.upstream)];
                (row.start..row.end, !row.last)
            }
            None => (0..self.content.len(), false),
        }
    }

    /// Command-Left and -Right (Home and End elsewhere): the start or end
    /// of the row the caret is on, which in wrapped text is not the line's.
    pub(super) fn line_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let (row, wrapped) = self.row_at(self.cursor_offset());
        if end {
            self.go(row.end, wrapped, extend, cx);
        } else {
            self.go(row.start, false, extend, cx);
        }
    }

    /// Control-A and -E: the start or end of the paragraph, the text
    /// between line breaks.
    pub(super) fn paragraph_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let target = if end {
            editing::paragraph_end(&self.content, cursor)
        } else {
            editing::paragraph_start(&self.content, cursor)
        };
        self.go(target, false, extend, cx);
    }

    /// Option-Up and -Down: to the start or end of the paragraph, or of
    /// the one before or after when already there.
    pub(super) fn paragraph_step(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let range = self.selected_range.clone();
        // With a selection and no Shift, the first step only collapses it
        // to its end (or start), as Option-Down does in production.
        let target = match (forward, extend || range.is_empty()) {
            (true, true) => editing::paragraph_end(&self.content, self.next_boundary(cursor)),
            (false, true) => {
                editing::paragraph_start(&self.content, self.previous_boundary(cursor))
            }
            (true, false) => editing::paragraph_end(&self.content, range.end),
            (false, false) => editing::paragraph_start(&self.content, range.start),
        };
        self.go(target, false, extend, cx);
    }

    pub(super) fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            cx.emit(EmptyBackspace);
            return;
        }
        let cursor = self.cursor_offset();
        self.delete_or_selection(
            self.previous_boundary(cursor)..cursor,
            Some(Typing::Delete),
            cx,
        );
    }

    pub(super) fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        self.delete_or_selection(cursor..self.next_boundary(cursor), Some(Typing::Delete), cx);
    }

    /// Control-Delete: the last code point of the character before the
    /// caret, so an accent comes off its letter.
    pub(super) fn delete_decomposing(
        &mut self,
        _: &DeleteBackwardDecomposing,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let start = editing::decomposed_backward(&self.content, cursor);
        self.delete_or_selection(start..cursor, Some(Typing::Delete), cx);
    }

    pub(super) fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let start = editing::word_start_before(&self.content, cursor);
        self.delete_or_selection(start..cursor, None, cx);
    }

    pub(super) fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let end = editing::word_right(&self.content, cursor);
        self.delete_or_selection(cursor..end, None, cx);
    }

    /// Command-Delete: back to the start of the row, or the character
    /// before when already there.
    pub(super) fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let mut start = self.row_at(cursor).0.start;
        if start == cursor {
            start = self.previous_boundary(cursor);
        }
        self.delete_or_selection(start..cursor, None, cx);
    }

    /// Control-K: to the end of the paragraph, or the line break when
    /// already there, into the kill buffer that Control-Y yanks back.
    /// Kills one after another add to it.
    pub(super) fn kill(
        &mut self,
        _: &DeleteToParagraphEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            let mut end = editing::paragraph_end(&self.content, cursor);
            if end == cursor {
                end = self.next_boundary(cursor);
            }
            cursor..end
        } else {
            self.selected_range.clone()
        };
        if range.is_empty() {
            return;
        }
        let killed = self.content[range.clone()].to_owned();
        let adding = self.killing;
        KILL_BUFFER.with_borrow_mut(|buffer| {
            if adding {
                buffer.push_str(&killed);
            } else {
                *buffer = killed;
            }
        });
        self.edit(range, "", None, cx);
        self.killing = true;
    }

    pub(super) fn yank(&mut self, _: &Yank, _: &mut Window, cx: &mut Context<Self>) {
        let text = KILL_BUFFER.with_borrow(|buffer| buffer.clone());
        let text = if self.multiline {
            text
        } else {
            editing::one_line(&text)
        };
        if !text.is_empty() {
            self.edit(self.selected_range.clone(), &text, None, cx);
        }
    }

    /// Control-T: the characters either side of the caret swap.
    pub(super) fn transpose(&mut self, _: &Transpose, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            return;
        }
        if let Some((range, text, caret)) = editing::transpose(&self.content, self.cursor_offset())
        {
            self.edit(range, &text, None, cx);
            self.selected_range = caret..caret;
        }
    }

    /// Control-O in a field of several lines: a line break after the caret.
    pub(super) fn open_line(&mut self, _: &OpenLine, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            return;
        }
        let start = self.selected_range.start;
        self.edit(self.selected_range.clone(), "\n", None, cx);
        self.selected_range = start..start;
    }

    /// Takes back the last step of editing in the field. With nothing left
    /// to take back it does nothing: as in production
    /// (`runNativeEditHistory`), Undo in a text field is the field's alone
    /// and never reaches the library's history, so an extra ⌘Z while typing
    /// in a dialog cannot undo a library edit behind it.
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_some() {
            return;
        }
        if let Some(state) = self.history.undo(self.snapshot()) {
            self.restore(state, true, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_some() {
            return;
        }
        if let Some(state) = self.history.redo(self.snapshot()) {
            self.restore(state, false, cx);
        }
    }

    /// Up and Down in a field of one line, which have no binding there so
    /// that lists around the field keep them: to the start or the end.
    pub(super) fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if self.last_lines.is_some()
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        let target = match keystroke.key.as_str() {
            "up" if cfg!(target_os = "macos") => 0,
            "down" if cfg!(target_os = "macos") => self.content.len(),
            _ => return,
        };
        cx.stop_propagation();
        self.go(target, false, modifiers.shift, cx);
    }

    pub(super) fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    /// Pasted text keeps its line breaks in a field of several lines; a
    /// field of one line keeps what `<input>` does of it.
    pub(super) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline {
                editing::normalize_breaks(&text)
            } else {
                editing::one_line(&text)
            };
            self.edit(self.selected_range.clone(), &text, None, cx);
        }
    }

    /// Shift+Enter in a multi-line field: a line break at the caret.
    pub(super) fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.edit(self.selected_range.clone(), "\n", Some(Typing::Insert), cx);
        }
    }

    /// The caret one row up or down in wrapped text, keeping to the same
    /// distance from the left over rows that are shorter; past the first
    /// or last row, to the start or end.
    pub(super) fn vertical(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let from = if extend || self.selected_range.is_empty() {
            self.cursor_offset()
        } else if down {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        let Some(lines) = self.last_lines.as_ref() else {
            let end = if down { self.content.len() } else { 0 };
            self.go(end, false, extend, cx);
            return;
        };
        let rows = rows_of(lines, px(self.kind.line_height()));
        let row = row_index(&rows, from, self.upstream);
        let x = self.goal_x.unwrap_or_else(|| x_in(lines, &rows[row], from));
        let (target, upstream) = if !down && row == 0 {
            (0, false)
        } else if down && row + 1 >= rows.len() {
            (self.content.len(), false)
        } else {
            let next = if down { row + 1 } else { row - 1 };
            hit_row(lines, &rows[next], x)
        };
        self.go(target, upstream, extend, cx);
        self.goal_x = Some(x);
    }

    pub(super) fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, false, cx);
    }

    pub(super) fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, false, cx);
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.edit(self.selected_range.clone(), "", None, cx)
        }
    }

    pub(super) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.moved();
        cx.notify()
    }

    pub(super) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(super) fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp(offset);
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.moved();
        cx.notify()
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        editing::previous_grapheme(&self.content, offset)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        editing::next_grapheme(&self.content, offset)
    }
}
