// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! This file holds the editing commands of a text field: the commands that
//! move the caret by character, word, row and paragraph, the commands that
//! delete, the kill buffer, Undo and Redo, and the clipboard.
use std::ops::Range;

use super::editing::{self, Platform, Snapshot, Typing};
use super::element::{hit_row, row_index, rows_of, x_in};
use super::*;
use gpui::{ClipboardItem, KeyDownEvent};
thread_local! {
    /// The text that Control-K last cut, which Control-Y puts back. All
    /// fields share this one buffer, as the text fields of one app do on a
    /// Mac.
    static KILL_BUFFER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

impl TextInput {
    /// Returns the text and the selection in the form in which the history
    /// stores them.
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.content.to_string(),
            selection: self.selected_range.clone(),
        }
    }

    /// Returns the end of the selection that the caret is not at. That end
    /// stays in place when the selection is extended.
    pub(super) fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    /// Returns `offset` moved back so that it is not past the end of the
    /// text and not inside a character. An offset that was read from a
    /// layout drawn before the last edit can be either.
    pub(super) fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.content.len());
        while !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// Records that the caret moved or the selection changed without an
    /// edit. What is typed next then starts a new step in the history, the
    /// next Control-K replaces the kill buffer, and the caret is drawn at
    /// once and starts its blinking again.
    pub(super) fn moved(&mut self) {
        self.history.close();
        self.killing = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
    }

    /// Replaces `range` with `text` and records the edit in the history, as
    /// a step of its own or as part of the run of typing that it continues
    /// (`typing`). The caret is put after the new text.
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

    /// Replaces `range` with `text` without recording the edit in the
    /// history. The caret is put after the new text.
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

    /// Sets the text and the selection to `state`, a state from the
    /// history. With `reversed`, which Undo passes, the caret is put at the
    /// start of the selection, so the text that Undo brought back is
    /// selected from its end to its start.
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

    /// Moves the caret to `offset`. With `extend` it moves only the end of
    /// the selection that the caret is at, and the other end stays in
    /// place. When `upstream` is true and `offset` is a place where a line
    /// wraps, the caret is drawn at the end of the upper row and not at the
    /// start of the lower row.
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

    /// Moves the caret to the start of the word before it, or with `extend`
    /// extends the selection to there. Option-Left on a Mac and Control-Left
    /// on Windows and Linux call it. When text is selected, the move starts
    /// from the end of the selection that the caret is at.
    pub(super) fn word_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        // The words of a password field cannot be seen, so the caret goes
        // to the start of the text.
        let target = if self.masked {
            0
        } else {
            editing::word_start_before(&self.content, self.cursor_offset())
        };
        self.go(target, false, extend, cx);
    }

    pub(super) fn word_right(&mut self, extend: bool, cx: &mut Context<Self>) {
        let target = if self.masked {
            self.content.len()
        } else {
            editing::word_right(&self.content, self.cursor_offset(), Platform::CURRENT)
        };
        self.go(target, false, extend, cx);
    }

    /// Returns the row on which a caret at `offset` is drawn: the row's
    /// byte range, and whether the row ends at a wrap and not at the end of
    /// a line. When the text is drawn on one line, the row is the whole
    /// text.
    fn row_at(&self, offset: usize) -> (Range<usize>, bool) {
        match self.last_lines.as_ref() {
            Some(lines) => {
                let rows = rows_of(lines, px(self.line_height()));
                let row = &rows[row_index(&rows, offset, self.upstream)];
                (row.start..row.end, !row.last)
            }
            None => (0..self.content.len(), false),
        }
    }

    /// Moves the caret to the start of the row it is on, or with `end` to
    /// the end of that row. In wrapped text a row is not the same as a
    /// line. Command-Left and Command-Right on a Mac, and Home and End on
    /// Windows and Linux, call it.
    pub(super) fn line_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let (row, wrapped) = self.row_at(self.cursor_offset());
        if end {
            self.go(row.end, wrapped, extend, cx);
        } else {
            self.go(row.start, false, extend, cx);
        }
    }

    /// Moves the caret to the start of the paragraph it is in, or with
    /// `end` to the end of that paragraph. A paragraph is the text between
    /// two line breaks. Control-A and Control-E on a Mac call it.
    pub(super) fn paragraph_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let target = if end {
            editing::paragraph_end(&self.content, cursor)
        } else {
            editing::paragraph_start(&self.content, cursor)
        };
        self.go(target, false, extend, cx);
    }

    /// Moves the caret to the start of the paragraph it is in, or with
    /// `forward` to the end of that paragraph. When the caret is already
    /// there, it goes to the start of the paragraph before, or to the end
    /// of the paragraph after. Option-Up and Option-Down on a Mac call it.
    pub(super) fn paragraph_step(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let range = self.selected_range.clone();
        // When text is selected and Shift is not held, the move starts from
        // the end of the selection (from its start for a move backward) and
        // does not first step over a line break. A selection that ends at
        // the end of a paragraph is therefore only collapsed to that end.
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

    /// Deletes the last code point of the character before the caret, so
    /// that an accent is removed and its letter stays. Control-Delete on a
    /// Mac calls it.
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
        let start = if self.masked {
            0
        } else {
            editing::word_start_before(&self.content, cursor)
        };
        self.delete_or_selection(start..cursor, None, cx);
    }

    pub(super) fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let end = if self.masked {
            self.content.len()
        } else {
            editing::word_right(&self.content, cursor, Platform::CURRENT)
        };
        self.delete_or_selection(cursor..end, None, cx);
    }

    /// Deletes from the caret back to the start of its row. When the caret
    /// is already at the start of the row, it deletes the character before
    /// the caret. Command-Delete on a Mac calls it.
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

    /// Deletes from the caret to the end of its paragraph and puts the
    /// deleted text in the kill buffer, from which Control-Y puts it back.
    /// When the caret is already at the end of the paragraph, the line
    /// break is deleted. When text is selected, the selection is deleted
    /// instead. A Control-K directly after another adds its text to the
    /// kill buffer. Control-K on a Mac calls it.
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

    /// Makes the character before the caret and the character after it
    /// change places (`editing::transpose`). It does nothing while text is
    /// selected. Control-T on a Mac calls it.
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

    /// Puts a line break after the caret in a field that takes line breaks,
    /// and leaves the caret where it was. Control-O on a Mac calls it.
    pub(super) fn open_line(&mut self, _: &OpenLine, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            return;
        }
        let start = self.selected_range.start;
        self.edit(self.selected_range.clone(), "\n", None, cx);
        self.selected_range = start..start;
    }

    /// Takes back the last step of editing in the field. When there is no
    /// step left, it does nothing. Undo in a text field never reaches the
    /// library's history, so pressing Undo once too often while typing in a
    /// dialog cannot undo a change to the library.
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

    /// Handles Up and Down in a field whose text is drawn on one line. The
    /// two keys have no binding in such a field, so that a list around the
    /// field can bind them. Up moves the caret to the start of the text and
    /// Down moves it to the end. On Windows they do nothing here.
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
        let ends = Platform::CURRENT.up_down_reach_the_ends();
        let target = match keystroke.key.as_str() {
            "up" if ends => 0,
            "down" if ends => self.content.len(),
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

    /// Replaces the selection with the text on the clipboard. A field that
    /// takes line breaks keeps the line breaks of the pasted text. A
    /// one-line field keeps what `editing::one_line` returns for it.
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

    /// Puts a line break at the caret in a field that takes line breaks.
    /// Shift+Enter calls it.
    pub(super) fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.edit(self.selected_range.clone(), "\n", Some(Typing::Insert), cx);
        }
    }

    /// Moves the caret one row up, or with `down` one row down, in wrapped
    /// text. The caret keeps the same distance from the left edge, also
    /// after it has passed through rows that are shorter. From the first
    /// row a move up goes to the start of the text, and from the last row a
    /// move down goes to the end. On Windows the caret stays where it is in
    /// those two cases.
    pub(super) fn vertical(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let from = if extend || self.selected_range.is_empty() {
            self.cursor_offset()
        } else if down {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        // For a move past the first or the last row, this returns the end
        // of the text that the move goes to (`end`), or on Windows the
        // place the caret is at.
        let past = |end: usize| {
            if Platform::CURRENT.up_down_reach_the_ends() {
                end
            } else {
                from
            }
        };
        let Some(lines) = self.last_lines.as_ref() else {
            let end = past(if down { self.content.len() } else { 0 });
            self.go(end, false, extend, cx);
            return;
        };
        let rows = rows_of(lines, px(self.line_height()));
        let row = row_index(&rows, from, self.upstream);
        let x = self.goal_x.unwrap_or_else(|| x_in(lines, &rows[row], from));
        let (target, upstream) = if !down && row == 0 {
            (past(0), false)
        } else if down && row + 1 >= rows.len() {
            (past(self.content.len()), false)
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
        // The text of a password field is not copied.
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
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
