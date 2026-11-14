// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
use std::ops::Range;

use editing::{Snapshot, Typing};
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId,
    KeyBinding, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::*;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Quit,
        Commit,
        Cancel,
        Newline,
        Up,
        Down,
        SelectUp,
        SelectDown,
        SelectToStart,
        SelectToEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        ParagraphStart,
        ParagraphEnd,
        SelectParagraphStart,
        SelectParagraphEnd,
        ParagraphBackward,
        ParagraphForward,
        SelectParagraphBackward,
        SelectParagraphForward,
        DeleteWordBackward,
        DeleteWordForward,
        DeleteToLineStart,
        DeleteToParagraphEnd,
        DeleteBackwardDecomposing,
        Transpose,
        Yank,
        OpenLine,
        Undo,
        Redo,
    ]
);

thread_local! {
    /// What Control-K last cut, which Control-Y puts back: one buffer for
    /// every field, as the Mac keeps one per app.
    static KILL_BUFFER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// What a press selects as the pointer drags: characters, or the whole
/// words or lines a double- or triple-click started with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Granularity {
    Character,
    Word,
    Line,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    /// What the stored value belongs to, as the view that owns the field
    /// numbers it.
    field_key: Option<u64>,
    original: SharedString,
    kind: Kind,
    /// Enter submits the field and Escape is left to what holds it.
    submits: bool,
    /// How far the text is scrolled left to keep the caret in view.
    scroll: Pixels,
    /// When the caret last moved; it shows for 500 ms from then, and blinks.
    blink: std::time::Instant,
    blink_task: Option<gpui::Task<()>>,
    /// An in-place rename that has not yet been committed or cancelled.
    renaming: bool,
    /// The field takes line breaks (Shift+Enter) and, while focused text no
    /// longer fits its one line, wraps it over as many lines as it needs
    /// (`InspectorField`'s focus expansion).
    multiline: bool,
    /// Whether the text needs more than the field's one line: it holds a
    /// line break or is wider than the field.
    overflowing: bool,
    /// The wrapped lines last drawn while expanded: each line's first byte,
    /// its layout and its top edge below the text's top.
    last_lines: Option<Vec<(usize, gpui::WrappedLine, Pixels)>>,
    /// The scroll of the expanded field's area, which the owner draws.
    area: gpui::ScrollHandle,
    /// The caret offset the expanded area last scrolled to show.
    revealed: Option<usize>,
    /// A text area (`GlassTextarea`): its text is always drawn wrapped
    /// from the top of its box, and Enter is a line break.
    text_area: bool,
    /// The field's own Undo and Redo, with typing gathered into runs.
    history: editing::History,
    /// A caret at a wrap stands after the upper row's last character
    /// rather than before the lower row's first, as Command-Right leaves
    /// it.
    upstream: bool,
    /// The distance from the left that Up and Down keep to while they move
    /// through shorter rows.
    goal_x: Option<Pixels>,
    /// What the press being dragged selects by.
    granularity: Granularity,
    /// What the press selected before the drag, which the selection always
    /// keeps.
    drag_origin: Range<usize>,
    /// Where the held pointer was last, for scrolling while it is past the
    /// field's edge.
    drag_point: Option<Point<Pixels>>,
    autoscroll: Option<gpui::Task<()>>,
    /// The last edit was a kill (Control-K), which the next one adds to.
    killing: bool,
}

/// `.textarea`'s `line-height: 1.4` at 13px.
pub const AREA_LINE: f32 = 13. * 1.4;

/// The byte range of each line of `text`, without its line break.
pub fn hard_lines(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, _) in text.match_indices('\n') {
        lines.push(start..index);
        start = index + 1;
    }
    lines.push(start..text.len());
    lines
}

/// One row of wrapped text as drawn: its bytes in the whole text, its top
/// edge, where it starts along its unwrapped line, which line that is, and
/// whether the row ends that line (rather than at a wrap).
#[derive(Clone, Debug, PartialEq)]
struct Row {
    start: usize,
    end: usize,
    top: Pixels,
    x0: Pixels,
    line: usize,
    last: bool,
}

/// The rows of wrapped lines, top to bottom.
fn rows_of(lines: &[(usize, gpui::WrappedLine, Pixels)], line_height: Pixels) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, (start, line, top)) in lines.iter().enumerate() {
        let layout = &line.unwrapped_layout;
        let wraps: Vec<usize> = line
            .wrap_boundaries()
            .iter()
            .map(|wrap| layout.runs[wrap.run_ix].glyphs[wrap.glyph_ix].index)
            .collect();
        let mut row_start = 0;
        for (number, row_end) in wraps.iter().copied().chain([line.len()]).enumerate() {
            rows.push(Row {
                start: start + row_start,
                end: start + row_end,
                top: *top + line_height * number as f32,
                x0: layout.x_for_index(row_start),
                line: index,
                last: number == wraps.len(),
            });
            row_start = row_end;
        }
    }
    rows
}

/// The row a caret at `offset` is drawn on. At a wrap the offset is both
/// the end of the upper row and the start of the lower; it is on the lower
/// unless `upstream`.
fn row_index(rows: &[Row], offset: usize, upstream: bool) -> usize {
    rows.iter()
        .position(|row| {
            row.start <= offset
                && (offset < row.end || (offset == row.end && (row.last || upstream)))
        })
        .unwrap_or(rows.len().saturating_sub(1))
}

/// How far from the left of its row `offset` is drawn.
fn x_in(lines: &[(usize, gpui::WrappedLine, Pixels)], row: &Row, offset: usize) -> Pixels {
    let (start, line, _) = &lines[row.line];
    line.unwrapped_layout
        .x_for_index(offset.saturating_sub(*start))
        - row.x0
}

/// The caret position in a row nearest to `x` from its left, and whether
/// it is the row's end at a wrap.
fn hit_row(lines: &[(usize, gpui::WrappedLine, Pixels)], row: &Row, x: Pixels) -> (usize, bool) {
    let (start, line, _) = &lines[row.line];
    let index = start
        + line
            .unwrapped_layout
            .closest_index_for_x(x.max(px(0.)) + row.x0);
    let offset = index.clamp(row.start, row.end);
    (offset, offset == row.end && !row.last)
}

/// Where `offset` is drawn in wrapped lines, from the text's top left.
fn caret_in(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    rows: &[Row],
    offset: usize,
    upstream: bool,
) -> Option<(Pixels, Pixels)> {
    let row = rows.get(row_index(rows, offset, upstream))?;
    Some((x_in(lines, row, offset), row.top))
}

/// Which of the app's text fields this is, which decides its type. The frame
/// around it (fill, border, padding) is `ui::field`.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The toolbar search.
    Search,
    /// The sidebar's tree filter.
    Sidebar,
    /// The inspector's name and notes.
    Inspector,
    /// A form field at the UI font's 13px: the settings searches.
    Form,
    /// A number, at 12px and against the right edge (`CompactNumberInput`).
    Number,
    /// A sidebar row renamed in place (`Sidebar.module.css` `.renameInput`):
    /// the row's own type, on a line that fits the 20px field.
    SidebarRename,
    /// The search at the top of a popover (`LibrarySwitcherPopover`).
    Popover,
    /// A context menu's search row (`.searchInput`).
    Menu,
    /// A small field at 12px on a 26px line: the filter bar's ranges.
    Compact,
    /// A colour's hex code, in the monospaced font at 12px (`.hexField`).
    Mono,
    /// A dialog's text area (`GlassInput.module.css` `.textarea`): 13px on
    /// a 1.4 line.
    Area,
}

impl Kind {
    fn font(self) -> &'static str {
        match self {
            Kind::Search
            | Kind::Form
            | Kind::Number
            | Kind::Popover
            | Kind::Menu
            | Kind::Compact
            | Kind::Area => crate::theme::FONT_UI,
            Kind::Mono => crate::theme::FONT_MONO,
            Kind::Sidebar | Kind::Inspector | Kind::SidebarRename => crate::theme::FONT_INSPECTOR,
        }
    }
    fn size(self) -> f32 {
        match self {
            Kind::Search | Kind::Number | Kind::Compact | Kind::Mono => crate::theme::TEXT_SM,
            Kind::Sidebar
            | Kind::Inspector
            | Kind::Form
            | Kind::Popover
            | Kind::SidebarRename
            | Kind::Menu
            | Kind::Area => crate::theme::TEXT_MD,
        }
    }
    fn line_height(self) -> f32 {
        match self {
            Kind::Search => 22.,
            Kind::Sidebar => 26.,
            Kind::Inspector => 20.,
            Kind::Form | Kind::Number => self.size() + 2.,
            Kind::SidebarRename => crate::theme::SIDEBAR_RENAME_HEIGHT - 2.,
            Kind::Popover => 48.,
            Kind::Menu => 36.,
            Kind::Compact => 24.,
            Kind::Mono => 30.,
            Kind::Area => AREA_LINE,
        }
    }
    /// The popover's input has no `::placeholder` rule, so it keeps
    /// Chromium's; the others set the tertiary text colour.
    fn placeholder(self, theme: crate::theme::Theme) -> gpui::Hsla {
        match self {
            Kind::Popover => theme.placeholder().into(),
            _ => theme.text_tertiary().into(),
        }
    }
}

/// A stored field's new value, with the key it was set for.
pub struct Committed(pub u64, pub String);
impl gpui::EventEmitter<Committed> for TextInput {}
/// Enter was pressed in a field made with [`TextInput::dialog`].
pub struct Submitted;
impl gpui::EventEmitter<Submitted> for TextInput {}
/// Backspace was pressed in an empty field.
pub struct EmptyBackspace;
impl gpui::EventEmitter<EmptyBackspace> for TextInput {}

/// How an in-place rename ended: Enter or leaving the field commits the
/// trimmed text, Escape cancels (`useInlineRename.ts`).
pub enum Renamed {
    Commit(String),
    Cancel,
}
impl gpui::EventEmitter<Renamed> for TextInput {}

impl TextInput {
    /// The text and selection, as the history keeps them.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.content.to_string(),
            selection: self.selected_range.clone(),
        }
    }

    /// Where the selection is held from: the end the caret is not at.
    fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    /// An offset no further than the text and on a character's edge, for
    /// positions read from a layout drawn before the last edit.
    fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.content.len());
        while !self.content.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// The caret moved or the selection changed by itself: typing that
    /// follows starts a new step to undo, and a kill is not added to.
    fn moved(&mut self) {
        self.history.close();
        self.killing = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
    }

    /// Replaces `range` with `text`, as one step to undo, or as part of the
    /// run of typing it continues. The caret goes after the new text.
    fn edit(
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
    fn splice(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
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
    fn go(&mut self, offset: usize, upstream: bool, extend: bool, cx: &mut Context<Self>) {
        let offset = self.clamp(offset);
        if extend {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        self.upstream = upstream;
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_to_start(&mut self, _: &SelectToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    /// Option-Left (Control-Left elsewhere) moves from the caret's end of a
    /// selection, as Chromium does, rather than from its start.
    fn word_left(&mut self, extend: bool, cx: &mut Context<Self>) {
        let target = editing::word_start_before(&self.content, self.cursor_offset());
        self.go(target, false, extend, cx);
    }

    fn word_right(&mut self, extend: bool, cx: &mut Context<Self>) {
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
    fn line_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
        let (row, wrapped) = self.row_at(self.cursor_offset());
        if end {
            self.go(row.end, wrapped, extend, cx);
        } else {
            self.go(row.start, false, extend, cx);
        }
    }

    /// Control-A and -E: the start or end of the paragraph, the text
    /// between line breaks.
    fn paragraph_edge(&mut self, end: bool, extend: bool, cx: &mut Context<Self>) {
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
    fn paragraph_step(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
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

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
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

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        self.delete_or_selection(cursor..self.next_boundary(cursor), Some(Typing::Delete), cx);
    }

    /// Control-Delete: the last code point of the character before the
    /// caret, so an accent comes off its letter.
    fn delete_decomposing(
        &mut self,
        _: &DeleteBackwardDecomposing,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let start = editing::decomposed_backward(&self.content, cursor);
        self.delete_or_selection(start..cursor, Some(Typing::Delete), cx);
    }

    fn delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let start = editing::word_start_before(&self.content, cursor);
        self.delete_or_selection(start..cursor, None, cx);
    }

    fn delete_word_forward(
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
    fn delete_to_line_start(
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
    fn kill(&mut self, _: &DeleteToParagraphEnd, _: &mut Window, cx: &mut Context<Self>) {
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

    fn yank(&mut self, _: &Yank, _: &mut Window, cx: &mut Context<Self>) {
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
    fn transpose(&mut self, _: &Transpose, _: &mut Window, cx: &mut Context<Self>) {
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
    fn open_line(&mut self, _: &OpenLine, _: &mut Window, cx: &mut Context<Self>) {
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
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_some() {
            return;
        }
        if let Some(state) = self.history.undo(self.snapshot()) {
            self.restore(state, true, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_some() {
            return;
        }
        if let Some(state) = self.history.redo(self.snapshot()) {
            self.restore(state, false, cx);
        }
    }

    /// Where a press lands in the text: the nearest caret position, and
    /// whether it is after the upper line at a wrap.
    fn hit(&self, position: Point<Pixels>) -> (usize, bool) {
        if self.content.is_empty() {
            return (0, false);
        }
        let Some(bounds) = self.last_bounds else {
            return (0, false);
        };
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.kind.line_height());
            let rows = rows_of(lines, line_height);
            let y = position.y - bounds.top();
            let bottom = rows.last().map_or(line_height, |row| row.top + line_height);
            if y < px(0.) {
                return (0, false);
            }
            if y >= bottom {
                return (self.content.len(), false);
            }
            let row = rows.iter().rposition(|row| row.top <= y).unwrap_or(0);
            let (offset, upstream) = hit_row(lines, &rows[row], position.x - bounds.left());
            return (self.clamp(offset), upstream);
        }
        let Some(line) = self.last_layout.as_ref() else {
            return (0, false);
        };
        if position.y < bounds.top() {
            return (0, false);
        }
        if position.y > bounds.bottom() {
            return (self.content.len(), false);
        }
        let x = position.x - bounds.left() + self.scroll;
        (self.clamp(line.closest_index_for_x(x)), false)
    }

    /// The character under a point, which a double-click selects the word
    /// of: the one whose box holds the point, not the nearest edge.
    fn character_under(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        // Above or below the text, as a press there places the caret.
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.kind.line_height());
            let rows = rows_of(lines, line_height);
            let y = position.y - bounds.top();
            let row = &rows[rows.iter().rposition(|row| row.top <= y).unwrap_or(0)];
            let (start, line, _) = &lines[row.line];
            let x = position.x - bounds.left() + row.x0;
            let index = line
                .unwrapped_layout
                .index_for_x(x)
                .map_or(row.end, |index| start + index);
            return self.clamp(index.clamp(row.start, row.end));
        }
        let Some(line) = self.last_layout.as_ref() else {
            return 0;
        };
        let x = position.x - bounds.left() + self.scroll;
        self.clamp(line.index_for_x(x).unwrap_or(self.content.len()))
    }

    /// What a double- or triple-click selects around a character.
    fn unit_at(&self, granularity: Granularity, at: usize) -> Range<usize> {
        match granularity {
            Granularity::Character => at..at,
            Granularity::Word => editing::word_at(&self.content, at, !cfg!(target_os = "macos")),
            Granularity::Line if self.multiline => editing::paragraph_at(&self.content, at),
            Granularity::Line => 0..self.content.len(),
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let (offset, upstream) = self.hit(event.position);
        self.granularity = match event.click_count {
            0 | 1 => Granularity::Character,
            2 => Granularity::Word,
            _ => Granularity::Line,
        };
        if self.granularity == Granularity::Character {
            self.go(offset, upstream, event.modifiers.shift, cx);
            self.drag_origin = self.anchor()..self.anchor();
            return;
        }
        let unit = self.unit_at(self.granularity, self.character_under(event.position));
        self.move_to(unit.start, cx);
        self.select_to(unit.end, cx);
        self.drag_origin = unit;
    }

    /// A press on the field's frame around the text (its padding), which
    /// in production is part of the field: it places the caret as a press
    /// on the nearest text would.
    pub fn press_outside(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .last_bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }
        self.on_mouse_down(event, window, cx);
    }

    /// The pointer moved with the button held after a press in the field:
    /// the selection follows it by character, or by whole words or lines
    /// after a double- or triple-click.
    fn drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag_point = Some(position);
        let (offset, upstream) = self.hit(position);
        let origin = self.drag_origin.clone();
        match self.granularity {
            Granularity::Character => {
                self.select_to(offset, cx);
                self.upstream = upstream;
            }
            granularity if offset < origin.start => {
                let unit = self.unit_at(granularity, offset);
                self.move_to(origin.end, cx);
                self.select_to(unit.start.min(origin.start), cx);
            }
            granularity => {
                let unit = self.unit_at(
                    granularity,
                    self.previous_boundary(offset).max(origin.start),
                );
                self.move_to(origin.start, cx);
                self.select_to(unit.end.max(origin.end), cx);
            }
        }
        self.autoscroll(cx);
    }

    /// While the pointer is held past the field's edge, the selection goes
    /// on following it, and the text scrolls, every 50 ms.
    fn autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.outside() {
            self.autoscroll = None;
            return;
        }
        if self.autoscroll.is_some() {
            return;
        }
        self.autoscroll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let going = this.update(cx, |this: &mut TextInput, cx| {
                    let point = this.drag_point.filter(|_| this.is_selecting);
                    match point {
                        Some(point) if this.outside() => {
                            this.drag(point, cx);
                            true
                        }
                        _ => {
                            this.autoscroll = None;
                            false
                        }
                    }
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// Whether the held pointer is past the edge the text scrolls along.
    fn outside(&self) -> bool {
        let (Some(point), Some(bounds)) = (self.drag_point, self.last_bounds) else {
            return false;
        };
        if self.last_lines.is_some() {
            let area = self.area.bounds();
            let area = if area.size.height > px(0.) {
                area
            } else {
                bounds
            };
            point.y < area.top() || point.y > area.bottom()
        } else {
            point.x < bounds.left() || point.x > bounds.right()
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.end_drag();
    }

    fn end_drag(&mut self) {
        self.is_selecting = false;
        self.drag_point = None;
        self.autoscroll = None;
    }

    /// A secondary click takes focus and, on a Mac, selects the word under
    /// the pointer unless it is inside the selection; elsewhere it moves
    /// the caret there. Production shows no menu of its own in a field, so
    /// the click goes on to whatever holds the field.
    fn on_secondary_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let (offset, upstream) = self.hit(event.position);
        let range = &self.selected_range;
        if !range.is_empty() && range.start <= offset && offset <= range.end {
            return;
        }
        if cfg!(target_os = "macos") {
            let word = self.unit_at(Granularity::Word, self.character_under(event.position));
            self.move_to(word.start, cx);
            self.select_to(word.end, cx);
        } else {
            self.go(offset, upstream, false, cx);
        }
    }

    /// Up and Down in a field of one line, which have no binding there so
    /// that lists around the field keep them: to the start or the end.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    /// Pasted text keeps its line breaks in a field of several lines; a
    /// field of one line keeps what `<input>` does of it.
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
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
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.edit(self.selected_range.clone(), "\n", Some(Typing::Insert), cx);
        }
    }

    /// The caret one row up or down in wrapped text, keeping to the same
    /// distance from the left over rows that are shorter; past the first
    /// or last row, to the start or end.
    fn vertical(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
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

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, false, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.edit(self.selected_range.clone(), "", None, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.clamp(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.moved();
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
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

    fn offset_from_utf16(&self, offset: usize) -> usize {
        editing::utf8_offset(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        editing::previous_grapheme(&self.content, offset)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        editing::next_grapheme(&self.content, offset)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        // Typed text continues the run of typing; text an input method was
        // composing joins the step its composition began.
        if self.marked_range.is_none() {
            self.history.record(self.snapshot(), Some(Typing::Insert));
        }
        self.splice(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        if self.marked_range.is_none() {
            self.history.record(self.snapshot(), Some(Typing::Insert));
        }

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        // The selection the input method gives is within the new text.
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                range.start + editing::utf8_offset(new_text, selected.start)
                    ..range.start + editing::utf8_offset(new_text, selected.end)
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.kind.line_height());
            let rows = rows_of(lines, line_height);
            let (start_x, top) = caret_in(lines, &rows, range.start, false)?;
            let (end_x, end_top) = caret_in(lines, &rows, range.end, true)?;
            let end_x = if end_top == top { end_x } else { start_x };
            return Some(Bounds::from_corners(
                point(bounds.left() + start_x, bounds.top() + top),
                point(bounds.left() + end_x, bounds.top() + top + line_height),
            ));
        }
        let last_layout = self.last_layout.as_ref()?;
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start) - self.scroll,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end) - self.scroll,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        bounds.localize(&point)?;
        if self.last_lines.is_some() {
            let (index, _) = self.hit(point);
            return Some(self.offset_to_utf16(index));
        }
        let last_layout = self.last_layout.as_ref()?;
        // The line drawn may be the clipped or placeholder text, so only an
        // index inside the content counts.
        let utf8_index = last_layout
            .index_for_x(point.x - bounds.left() + self.scroll)
            .filter(|index| *index <= self.content.len())?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

enum PrepaintState {
    /// One line, scrolled sideways to keep the caret in view.
    Single {
        line: Option<ShapedLine>,
        cursor: Option<PaintQuad>,
        selection: Option<PaintQuad>,
        scroll: Pixels,
    },
    /// An expanded field's text, wrapped over as many lines as it needs.
    Wrapped {
        lines: Vec<(usize, gpui::WrappedLine, Pixels)>,
        cursor: Option<PaintQuad>,
        selection: Vec<PaintQuad>,
        /// Where the selection's moving end is, which the expanded area
        /// keeps in view.
        focus: Option<Bounds<Pixels>>,
    },
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The runs for `text`, with the input method's marked range underlined.
fn runs_for(
    text: &str,
    font: gpui::Font,
    color: gpui::Hsla,
    marked_range: Option<&Range<usize>>,
) -> Vec<TextRun> {
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    match marked_range {
        Some(marked) if marked.end <= text.len() => vec![
            TextRun {
                len: marked.start,
                ..run.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..run.clone()
            },
            TextRun {
                len: text.len() - marked.end,
                ..run
            },
        ]
        .into_iter()
        .filter(|run| run.len > 0)
        .collect(),
        _ => vec![run],
    }
}

/// `text` wrapped to `width`: each line's first byte, its layout and its top
/// edge, and the height of them all.
fn wrap(
    text: &SharedString,
    font: gpui::Font,
    font_size: Pixels,
    color: gpui::Hsla,
    marked_range: Option<&Range<usize>>,
    width: Option<Pixels>,
    line_height: Pixels,
    window: &mut Window,
) -> (Vec<(usize, gpui::WrappedLine, Pixels)>, Pixels) {
    let runs = runs_for(text, font, color, marked_range);
    let shaped = window
        .text_system()
        .shape_text(text.clone(), font_size, &runs, width, None)
        .unwrap_or_default();
    let mut lines = Vec::with_capacity(shaped.len());
    let mut top = px(0.);
    for (range, line) in hard_lines(text).into_iter().zip(shaped) {
        let height = line.size(line_height).height.max(line_height);
        lines.push((range.start, line, top));
        top += height;
    }
    (lines, top.max(line_height))
}

impl TextElement {
    /// Whether the input is drawn wrapped this frame.
    fn wrapped(&self, window: &Window, cx: &App) -> bool {
        let input = self.input.read(cx);
        input.multiline
            && ((input.text_area && !input.content.is_empty())
                || (input.overflowing && input.focus_handle.is_focused(window)))
    }

    fn prepaint_wrapped(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let marked_range = input.marked_range.clone();
        let style = window.text_style();
        let theme = crate::theme::theme(cx);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let (lines, _) = wrap(
            &content,
            style.font(),
            font_size,
            style.color,
            marked_range.as_ref(),
            Some(bounds.size.width),
            line_height,
            window,
        );
        let rows = rows_of(&lines, line_height);
        let overflowing = rows.len() > 1;
        let focused = input.focus_handle.is_focused(window);
        let upstream = input.upstream;
        let glyphs = font_size * 1.2;
        let focus = caret_in(&lines, &rows, cursor, upstream)
            .filter(|_| focused)
            .map(|(x, y)| {
                Bounds::new(
                    point(bounds.left() + x, bounds.top() + y),
                    size(px(1.), line_height),
                )
            });
        let (cursor, selection) = if !focused {
            (None, Vec::new())
        } else if selected_range.is_empty() {
            let quad = caret_in(&lines, &rows, cursor, upstream).map(|(x, y)| {
                fill(
                    Bounds::new(
                        point(
                            bounds.left() + x,
                            bounds.top() + y + (line_height - glyphs) / 2.,
                        ),
                        size(px(1.), glyphs),
                    ),
                    theme.text(),
                )
            });
            (quad, Vec::new())
        } else {
            // Chromium's selection: each row from where the selection
            // starts in it to where it ends, and a space's width more where
            // it runs on through a line break.
            let space = window
                .text_system()
                .shape_line(
                    " ".into(),
                    font_size,
                    &runs_for(" ", style.font(), style.color, None),
                    None,
                )
                .width;
            let quads = rows
                .iter()
                .filter(|row| row.end >= selected_range.start && row.start <= selected_range.end)
                .filter_map(|row| {
                    let from = selected_range.start.max(row.start);
                    let to = selected_range.end.min(row.end);
                    let left = x_in(&lines, row, from);
                    let mut right = x_in(&lines, row, to);
                    if row.last && selected_range.end > row.end {
                        right += space;
                    }
                    (right > left).then(|| {
                        fill(
                            Bounds::from_corners(
                                point(bounds.left() + left, bounds.top() + row.top),
                                point(bounds.left() + right, bounds.top() + row.top + line_height),
                            ),
                            theme.selection(),
                        )
                    })
                })
                .collect();
            (None, quads)
        };
        self.input.update(cx, |input, cx| {
            if input.overflowing != overflowing {
                input.overflowing = overflowing;
                cx.notify();
            }
        });
        PrepaintState::Wrapped {
            lines,
            cursor,
            selection,
            focus,
        }
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !self.wrapped(window, cx) {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        // Expanded: as tall as the text wraps at the width it is given.
        let input = self.input.read(cx);
        let text = input.content.clone();
        let marked = input.marked_range.clone();
        let text_style = window.text_style();
        let font = text_style.font();
        let color = text_style.color;
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let (_, height) = wrap(
                &text,
                font.clone(),
                font_size,
                color,
                marked.as_ref(),
                width,
                line_height,
                window,
            );
            size(width.unwrap_or_default(), height)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.wrapped(window, cx) {
            return self.prepaint_wrapped(bounds, window, cx);
        }
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let focused = input.focus_handle.is_focused(window);
        let marked_range = input.marked_range.clone();
        let placeholder = input.placeholder.clone();
        let kind = input.kind;
        let multiline = input.multiline;
        let previous_scroll = input.scroll;
        let style = window.text_style();
        let theme = crate::theme::theme(cx);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let shape = |text: SharedString, color: gpui::Hsla, window: &mut Window| {
            let runs = runs_for(&text, style.font(), color, marked_range.as_ref());
            window
                .text_system()
                .shape_line(text, font_size, &runs, None)
        };

        let (display_text, text_color) = if content.is_empty() {
            (placeholder, kind.placeholder(theme))
        } else if content.contains(['\n', '\r']) {
            // This element shapes one line, and a line break panics the shaper.
            // A space is the same byte length, so cursor offsets stay valid and
            // the stored value keeps its real line breaks.
            (
                SharedString::from(content.replace(['\n', '\r'], " ")),
                style.color,
            )
        } else {
            (content.clone(), style.color)
        };
        let mut line = shape(display_text.clone(), text_color, window);
        // An expanding field notes when its text needs more than one line
        // (`measureOverflow`), so its owner can expand it while focused.
        let overflowing = !content.is_empty()
            && (content.contains(['\n', '\r']) || line.width > bounds.size.width);
        if multiline && input.overflowing != overflowing {
            self.input.update(cx, |input, cx| {
                input.overflowing = overflowing;
                cx.notify();
            });
        }

        // A field that is not being edited shows its start and an ellipsis,
        // as `text-overflow: ellipsis` does.
        if !focused && line.width > bounds.size.width && !content.is_empty() {
            let ellipsis = shape("…".into(), text_color, window).width;
            let fits = display_text
                .grapheme_indices(true)
                .map(|(index, _)| index)
                .take_while(|index| line.x_for_index(*index) + ellipsis <= bounds.size.width)
                .last()
                .unwrap_or(0);
            let clipped = SharedString::from(format!("{}…", &display_text[..fits]));
            line = shape(clipped, text_color, window);
        }

        // While editing, the text scrolls just far enough to show the caret.
        let cursor_x = line.x_for_index(cursor.min(line.len()));
        let caret_room = bounds.size.width - px(1.);
        let right_aligned = kind == Kind::Number;
        let scroll = if right_aligned && line.width <= caret_room {
            // Text set against the right edge: a negative scroll moves it
            // there, and the caret, selection and hit testing follow.
            line.width - caret_room
        } else if !focused || line.width <= caret_room {
            px(0.)
        } else {
            previous_scroll
                .min(cursor_x)
                .max(cursor_x - caret_room)
                .min(line.width - caret_room)
                .max(px(0.))
        };
        if scroll != previous_scroll {
            self.input.update(cx, |input, _| input.scroll = scroll);
        }

        // Chromium's caret: one pixel wide and as tall as the glyphs.
        let glyphs = line.ascent + line.descent;
        let caret_top = bounds.top() + (bounds.size.height - glyphs) / 2.;
        let left = bounds.left() - scroll;
        let (selection, cursor) = if !focused {
            (None, None)
        } else if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(point(left + cursor_x, caret_top), size(px(1.), glyphs)),
                    theme.text(),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected_range.start), bounds.top()),
                        point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                    ),
                    theme.selection(),
                )),
                None,
            )
        };
        PrepaintState::Single {
            line: Some(line),
            cursor,
            selection,
            scroll,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        // The caret is on for the first half of each second since it moved.
        let since_move = self.input.read(cx).blink.elapsed();
        let caret_on = (since_move.as_millis() / crate::motion::CARET_BLINK.as_millis()) % 2 == 0;
        let (single, wrapped) = match prepaint {
            PrepaintState::Single {
                line,
                cursor,
                selection,
                scroll,
            } => {
                let line = line.take().unwrap();
                let origin = point(bounds.left() - *scroll, bounds.top());
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    if let Some(selection) = selection.take() {
                        window.paint_quad(selection)
                    }
                    line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx)
                        .unwrap();
                    if let Some(cursor) = cursor.take().filter(|_| caret_on) {
                        window.paint_quad(cursor);
                    }
                });
                (Some(line), None)
            }
            PrepaintState::Wrapped {
                lines,
                cursor,
                selection,
                focus,
            } => {
                for quad in selection.drain(..) {
                    window.paint_quad(quad);
                }
                for (_, line, top) in lines.iter() {
                    let origin = point(bounds.left(), bounds.top() + *top);
                    let _ = line.paint(
                        origin,
                        line_height,
                        gpui::TextAlign::Left,
                        Some(bounds),
                        window,
                        cx,
                    );
                }
                if let Some(cursor) = cursor.take().filter(|_| caret_on) {
                    window.paint_quad(cursor);
                }
                (None, Some((std::mem::take(lines), focus.take())))
            }
        };

        // A press in the field goes on selecting wherever the pointer goes,
        // in the window or past it, while the button is held.
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !input.read(cx).is_selecting {
                return;
            }
            input.update(cx, |input, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    input.drag(event.position, cx);
                } else {
                    input.end_drag();
                }
            });
        });

        let focused = focus_handle.is_focused(window);
        self.input.update(cx, |input, cx| {
            input.last_bounds = Some(bounds);
            match (single, wrapped) {
                (Some(line), _) => {
                    input.last_layout = Some(line);
                    input.last_lines = None;
                }
                (_, Some((lines, caret))) => {
                    input.last_layout = None;
                    input.last_lines = Some(lines);
                    // The expanded area scrolls to keep a moved caret in view.
                    let offset = input.cursor_offset();
                    if let Some(caret) = caret
                        && input.revealed != Some(offset)
                    {
                        input.revealed = Some(offset);
                        let area = input.area.bounds();
                        let top = caret.top() - px(6.);
                        let bottom = caret.bottom() + px(6.);
                        let mut scroll = input.area.offset();
                        if bottom > area.bottom() {
                            scroll.y -= bottom - area.bottom();
                        } else if top < area.top() {
                            scroll.y += area.top() - top;
                        }
                        if scroll != input.area.offset() {
                            input.area.set_offset(scroll);
                            cx.notify();
                        }
                    }
                }
                _ => {}
            }
            // Repaint at the caret's next change for as long as it shows.
            if focused && input.blink_task.is_none() {
                let wait = crate::motion::CARET_BLINK.as_millis()
                    - since_move.as_millis() % crate::motion::CARET_BLINK.as_millis();
                input.blink_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(wait as u64 + 1))
                        .await;
                    let _ = this.update(cx, |this: &mut TextInput, cx| {
                        this.blink_task = None;
                        cx.notify();
                    });
                }));
            }
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Expanded, the text is as tall as it wraps; otherwise one line,
        // centred in the field.
        let root = if self.expanded(window) {
            div().w_full()
        } else if self.text_area {
            // From the top of the box, filling it, so a press below the
            // text still lands in the field.
            div().size_full()
        } else {
            div().size_full().flex().items_center()
        };
        #[cfg(test)]
        {
            let name = format!("field \"{}\"", self.placeholder);
            crate::ui::focus::names::name_text(&self.focus_handle(cx), name, cx);
        }
        root.key_context(if self.multiline {
            "TextInput TextArea"
        } else {
            "TextInput"
        })
        .track_focus(&self.focus_handle(cx))
        .cursor(CursorStyle::IBeam)
        .on_action(cx.listener(Self::backspace))
        .on_action(cx.listener(Self::delete))
        .on_action(cx.listener(Self::left))
        .on_action(cx.listener(Self::right))
        .on_action(cx.listener(Self::select_left))
        .on_action(cx.listener(Self::select_right))
        .on_action(cx.listener(Self::select_all))
        .on_action(cx.listener(Self::home))
        .on_action(cx.listener(Self::end))
        .on_action(cx.listener(Self::show_character_palette))
        .on_action(cx.listener(Self::paste))
        .on_action(cx.listener(Self::cut))
        .on_action(cx.listener(Self::copy))
        .on_action(cx.listener(Self::newline))
        .on_action(cx.listener(Self::up))
        .on_action(cx.listener(Self::down))
        .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(false, true, cx)))
        .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(true, true, cx)))
        .on_action(cx.listener(Self::select_to_start))
        .on_action(cx.listener(Self::select_to_end))
        .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.word_left(false, cx)))
        .on_action(cx.listener(|this, _: &WordRight, _, cx| this.word_right(false, cx)))
        .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.word_left(true, cx)))
        .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.word_right(true, cx)))
        .on_action(cx.listener(|this, _: &LineStart, _, cx| this.line_edge(false, false, cx)))
        .on_action(cx.listener(|this, _: &LineEnd, _, cx| this.line_edge(true, false, cx)))
        .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| this.line_edge(false, true, cx)))
        .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| this.line_edge(true, true, cx)))
        .on_action(
            cx.listener(|this, _: &ParagraphStart, _, cx| this.paragraph_edge(false, false, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ParagraphEnd, _, cx| this.paragraph_edge(true, false, cx)),
        )
        .on_action(
            cx.listener(|this, _: &SelectParagraphStart, _, cx| {
                this.paragraph_edge(false, true, cx)
            }),
        )
        .on_action(
            cx.listener(|this, _: &SelectParagraphEnd, _, cx| this.paragraph_edge(true, true, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ParagraphBackward, _, cx| this.paragraph_step(false, false, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ParagraphForward, _, cx| this.paragraph_step(true, false, cx)),
        )
        .on_action(cx.listener(|this, _: &SelectParagraphBackward, _, cx| {
            this.paragraph_step(false, true, cx)
        }))
        .on_action(cx.listener(|this, _: &SelectParagraphForward, _, cx| {
            this.paragraph_step(true, true, cx)
        }))
        .on_action(cx.listener(Self::delete_word_backward))
        .on_action(cx.listener(Self::delete_word_forward))
        .on_action(cx.listener(Self::delete_to_line_start))
        .on_action(cx.listener(Self::kill))
        .on_action(cx.listener(Self::delete_decomposing))
        .on_action(cx.listener(Self::transpose))
        .on_action(cx.listener(Self::yank))
        .on_action(cx.listener(Self::open_line))
        .on_action(cx.listener(Self::undo))
        .on_action(cx.listener(Self::redo))
        .on_key_down(cx.listener(Self::on_key_down))
        .on_action(cx.listener(|this, _: &Commit, window, cx| {
            if this.text_area {
                this.edit(this.selected_range.clone(), "\n", Some(Typing::Insert), cx);
                return;
            }
            if this.submits {
                cx.emit(Submitted);
                return;
            }
            this.commit(cx);
            // An in-place rename ends here rather than waiting for the blur,
            // which a window that is not active never reports.
            this.finish_rename(true, cx);
            window.blur(cx);
        }))
        .on_action(cx.listener(|this, _: &Cancel, window, cx| {
            // A dialog or screen decides what Escape does in its field.
            if this.submits {
                cx.propagate();
                return;
            }
            // The sidebar filter empties on Escape and keeps the keyboard,
            // as the Electron input does.
            if this.kind == Kind::Sidebar {
                if !this.content.is_empty() {
                    this.content = "".into();
                    this.selected_range = 0..0;
                    cx.notify();
                }
                return;
            }
            this.finish_rename(false, cx);
            // A stored field goes back to its saved value.
            this.content = this.original.clone();
            this.selected_range = 0..0;
            this.selection_reversed = false;
            this.history.clear();
            this.moved();
            window.blur(cx);
            cx.notify();
        }))
        .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(Self::on_secondary_mouse_down),
        )
        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
        .font_family(self.kind.font())
        .line_height(px(self.kind.line_height()))
        .text_size(px(self.kind.size()))
        .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            // A field is a stop for Tab, as an `<input>` is.
            focus_handle: cx.focus_handle().tab_stop(true),
            content: "".into(),
            placeholder: "Search".into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            field_key: None,
            original: "".into(),
            kind: Kind::Search,
            submits: false,
            scroll: px(0.),
            blink: std::time::Instant::now(),
            blink_task: None,
            renaming: false,
            multiline: false,
            overflowing: false,
            last_lines: None,
            area: gpui::ScrollHandle::new(),
            revealed: None,
            text_area: false,
            history: editing::History::default(),
            upstream: false,
            goal_x: None,
            granularity: Granularity::Character,
            drag_origin: 0..0,
            drag_point: None,
            autoscroll: None,
            killing: false,
        }
    }
    /// The sidebar's tree filter.
    pub fn sidebar(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Sidebar;
        input
    }
    /// A sidebar row's in-place rename, holding the current name selected.
    /// Losing focus commits it.
    pub fn sidebar_rename(value: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = "".into();
        input.kind = Kind::SidebarRename;
        input.content = value.to_owned().into();
        input.original = input.content.clone();
        input.selected_range = 0..input.content.len();
        input.renaming = true;
        cx.on_blur(&input.focus_handle, window, |this, _, cx| {
            this.finish_rename(true, cx)
        })
        .detach();
        input
    }
    /// Ends an in-place rename once: committed, or cancelled.
    pub fn finish_rename(&mut self, commit: bool, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.renaming) {
            return;
        }
        cx.emit(if commit {
            Renamed::Commit(self.content.trim().to_owned())
        } else {
            Renamed::Cancel
        });
    }
    /// The search field of a popover.
    pub fn popover(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Popover;
        input
    }
    /// A context menu's search field. The menu takes its Enter, Escape and
    /// arrows before the field sees them.
    pub fn menu_search(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = "Search...".into();
        input.kind = Kind::Menu;
        input
    }
    /// The search of the Add Tags and Folders dialogs (`.searchInput`): a
    /// menu search's type with its own placeholder.
    pub fn list_search(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::menu_search(cx);
        input.placeholder = placeholder.into();
        input
    }
    pub fn select_everything(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        self.moved();
        cx.notify();
    }
    /// The caret after the last character, where a browser leaves it in a
    /// field given its value before it takes focus.
    pub fn caret_to_end(&mut self, cx: &mut Context<Self>) {
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.moved();
        cx.notify();
    }
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
    /// Empties the field. Text replaced from outside the field takes its
    /// history with it.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.history.clear();
        self.moved();
        cx.notify();
    }
    pub fn value(&self) -> String {
        self.content.to_string()
    }
    /// A form field whose Enter submits it ([`Submitted`]) and whose Escape
    /// is left to what holds it: dialog fields and the Tags screen's search.
    pub fn dialog(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::form(placeholder, cx);
        input.submits = true;
        input
    }
    /// A field set into an inspector row, in the inspector's type, whose
    /// Enter submits it ([`Submitted`]) and whose Escape is left to what
    /// holds it: the source list's address editor.
    pub fn inline(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Inspector;
        input.submits = true;
        input
    }
    /// A form field, at the UI font's 13px.
    pub fn form(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Form;
        input
    }
    /// A dialog's text area (`GlassTextarea`), wrapped from the top of its
    /// box, where Enter starts a new line.
    pub fn text_area(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Area;
        input.multiline = true;
        input.text_area = true;
        // Escape is left to the dialog.
        input.submits = true;
        input
    }
    /// A small field of the filter bar's editors, at 12px.
    pub fn compact(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Compact;
        input
    }
    /// A colour's hex code.
    pub fn mono(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = "".into();
        input.kind = Kind::Mono;
        input
    }
    /// A number field, set against its right edge.
    pub fn number(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = "".into();
        input.kind = Kind::Number;
        input
    }
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }
    /// Puts the caret after the text, where a browser's focus leaves it.
    pub fn caret_at_end(&mut self, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    /// Replaces the text, which Escape then goes back to.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.original = text.clone().into();
        self.content = text.into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.history.clear();
        self.moved();
        cx.notify();
    }
    pub fn field(placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Inspector;
        cx.on_blur(&input.focus_handle, window, |this, _, cx| {
            this.commit(cx);
            // An expanded field goes back to its one line.
            cx.notify();
        })
        .detach();
        input
    }
    /// An inspector field that takes line breaks with Shift+Enter and, while
    /// focused text overflows its line, is drawn expanded by its frame
    /// (`InspectorField`).
    pub fn expanding(
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input = Self::field(placeholder, window, cx);
        input.multiline = true;
        input
    }
    /// Whether the field is focused and its text needs more than one line,
    /// so its frame draws it expanded over what follows.
    pub fn expanded(&self, window: &Window) -> bool {
        self.multiline && self.overflowing && self.focus_handle.is_focused(window)
    }
    /// The scroll of the expanded field's area.
    pub fn area(&self) -> &gpui::ScrollHandle {
        &self.area
    }
    pub fn set_field(&mut self, key: u64, value: String, cx: &mut Context<Self>) {
        if self.field_key == Some(key) && self.content != self.original {
            return;
        }
        if self.field_key != Some(key) || self.content.as_ref() != value {
            self.history.clear();
        }
        self.field_key = Some(key);
        self.original = value.clone().into();
        self.content = value.into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.moved();
        cx.notify();
    }
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.field_key {
            let value = self.content.trim().to_owned();
            if value != self.original.as_ref() {
                self.original = value.clone().into();
                cx.emit(Committed(key, value));
            }
        }
    }
}
/// The keys of a text field: AppKit's standard key bindings on a Mac (as
/// Chromium performs them in production's fields), the Windows and Linux
/// ones elsewhere. Undo and Redo come from the shortcut registry
/// (`keymap.rs`), which follows the user's keys for them.
pub fn bind(cx: &mut App) {
    let field = Some("TextInput");
    let area = Some("TextArea");
    cx.bind_keys([
        KeyBinding::new("enter", Commit, field),
        KeyBinding::new("shift-enter", Newline, area),
        KeyBinding::new("up", Up, area),
        KeyBinding::new("down", Down, area),
        KeyBinding::new("shift-up", SelectUp, area),
        KeyBinding::new("shift-down", SelectDown, area),
        KeyBinding::new("escape", Cancel, field),
        KeyBinding::new("backspace", Backspace, field),
        KeyBinding::new("delete", Delete, field),
        KeyBinding::new("left", Left, field),
        KeyBinding::new("right", Right, field),
        KeyBinding::new("shift-left", SelectLeft, field),
        KeyBinding::new("shift-right", SelectRight, field),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-a", SelectAll, field),
            KeyBinding::new("cmd-v", Paste, field),
            KeyBinding::new("cmd-c", Copy, field),
            KeyBinding::new("cmd-x", Cut, field),
            KeyBinding::new("home", Home, field),
            KeyBinding::new("end", End, field),
            KeyBinding::new("shift-home", SelectToStart, field),
            KeyBinding::new("shift-end", SelectToEnd, field),
            KeyBinding::new("cmd-up", Home, field),
            KeyBinding::new("cmd-down", End, field),
            KeyBinding::new("cmd-shift-up", SelectToStart, field),
            KeyBinding::new("cmd-shift-down", SelectToEnd, field),
            KeyBinding::new("alt-left", WordLeft, field),
            KeyBinding::new("alt-right", WordRight, field),
            KeyBinding::new("alt-shift-left", SelectWordLeft, field),
            KeyBinding::new("alt-shift-right", SelectWordRight, field),
            KeyBinding::new("ctrl-alt-b", WordLeft, field),
            KeyBinding::new("ctrl-alt-f", WordRight, field),
            KeyBinding::new("ctrl-alt-shift-b", SelectWordLeft, field),
            KeyBinding::new("ctrl-alt-shift-f", SelectWordRight, field),
            KeyBinding::new("cmd-left", LineStart, field),
            KeyBinding::new("cmd-right", LineEnd, field),
            KeyBinding::new("cmd-shift-left", SelectLineStart, field),
            KeyBinding::new("cmd-shift-right", SelectLineEnd, field),
            KeyBinding::new("ctrl-left", LineStart, field),
            KeyBinding::new("ctrl-right", LineEnd, field),
            KeyBinding::new("ctrl-shift-left", SelectLineStart, field),
            KeyBinding::new("ctrl-shift-right", SelectLineEnd, field),
            KeyBinding::new("alt-up", ParagraphBackward, field),
            KeyBinding::new("alt-down", ParagraphForward, field),
            KeyBinding::new("alt-shift-up", SelectParagraphBackward, field),
            KeyBinding::new("alt-shift-down", SelectParagraphForward, field),
            // The Emacs keys every Mac text field takes.
            KeyBinding::new("ctrl-a", ParagraphStart, field),
            KeyBinding::new("ctrl-e", ParagraphEnd, field),
            KeyBinding::new("ctrl-shift-a", SelectParagraphStart, field),
            KeyBinding::new("ctrl-shift-e", SelectParagraphEnd, field),
            KeyBinding::new("ctrl-b", Left, field),
            KeyBinding::new("ctrl-f", Right, field),
            KeyBinding::new("ctrl-shift-b", SelectLeft, field),
            KeyBinding::new("ctrl-shift-f", SelectRight, field),
            KeyBinding::new("ctrl-p", Up, field),
            KeyBinding::new("ctrl-n", Down, field),
            KeyBinding::new("ctrl-shift-p", SelectUp, field),
            KeyBinding::new("ctrl-shift-n", SelectDown, field),
            KeyBinding::new("ctrl-h", Backspace, field),
            KeyBinding::new("ctrl-d", Delete, field),
            KeyBinding::new("ctrl-k", DeleteToParagraphEnd, field),
            KeyBinding::new("ctrl-y", Yank, field),
            KeyBinding::new("ctrl-t", Transpose, field),
            KeyBinding::new("ctrl-o", OpenLine, field),
            KeyBinding::new("alt-backspace", DeleteWordBackward, field),
            KeyBinding::new("ctrl-alt-backspace", DeleteWordBackward, field),
            KeyBinding::new("alt-delete", DeleteWordForward, field),
            KeyBinding::new("cmd-backspace", DeleteToLineStart, field),
            KeyBinding::new("ctrl-backspace", DeleteBackwardDecomposing, field),
            KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, field),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-a", SelectAll, field),
            KeyBinding::new("ctrl-v", Paste, field),
            KeyBinding::new("shift-insert", Paste, field),
            KeyBinding::new("ctrl-c", Copy, field),
            KeyBinding::new("ctrl-insert", Copy, field),
            KeyBinding::new("ctrl-x", Cut, field),
            KeyBinding::new("shift-delete", Cut, field),
            KeyBinding::new("home", LineStart, field),
            KeyBinding::new("end", LineEnd, field),
            KeyBinding::new("shift-home", SelectLineStart, field),
            KeyBinding::new("shift-end", SelectLineEnd, field),
            KeyBinding::new("ctrl-home", Home, field),
            KeyBinding::new("ctrl-end", End, field),
            KeyBinding::new("ctrl-shift-home", SelectToStart, field),
            KeyBinding::new("ctrl-shift-end", SelectToEnd, field),
            KeyBinding::new("ctrl-left", WordLeft, field),
            KeyBinding::new("ctrl-right", WordRight, field),
            KeyBinding::new("ctrl-shift-left", SelectWordLeft, field),
            KeyBinding::new("ctrl-shift-right", SelectWordRight, field),
            KeyBinding::new("ctrl-backspace", DeleteWordBackward, field),
            KeyBinding::new("ctrl-delete", DeleteWordForward, field),
        ]);
    }
}

/// The editing rules of the browser's text fields on a Mac (and their
/// Windows and Linux equivalents), on byte offsets into the text and with
/// no layout: graphemes, words as Chromium finds them, paragraphs, what a
/// one-line field keeps of pasted text, and the history Undo walks.
pub mod editing {
    use std::ops::Range;
    use unicode_segmentation::UnicodeSegmentation;

    pub fn previous_grapheme(text: &str, offset: usize) -> usize {
        text.grapheme_indices(true)
            .rev()
            .map(|(index, _)| index)
            .find(|index| *index < offset)
            .unwrap_or(0)
    }

    pub fn next_grapheme(text: &str, offset: usize) -> usize {
        text.grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|index| *index > offset)
            .unwrap_or(text.len())
    }

    /// The text cut into words, runs of space and marks, as Chromium cuts
    /// it: Unicode's word rules with the dictionaries for Chinese, Japanese
    /// and Thai, except that a full stop parts letters (`example.com`) and
    /// joins only digits (`3.14`), and a colon parts everything.
    pub fn segments(text: &str) -> Vec<Range<usize>> {
        let mut bounds: Vec<usize> =
            icu_segmenter::WordSegmenter::new_dictionary(Default::default())
                .segment_str(text)
                .collect();
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let digit = |at: Option<&(usize, char)>| at.is_some_and(|(_, c)| c.is_numeric());
        for (n, &(index, c)) in chars.iter().enumerate() {
            let stop = matches!(c, '.' | '\u{FE52}' | '\u{FF0E}');
            let colon = matches!(c, ':' | '\u{FE55}' | '\u{FF1A}');
            let between_digits = n > 0 && digit(chars.get(n - 1)) && digit(chars.get(n + 1));
            if colon || (stop && !between_digits) {
                bounds.push(index);
                bounds.push(index + c.len_utf8());
            }
        }
        bounds.push(0);
        bounds.push(text.len());
        bounds.sort_unstable();
        bounds.dedup();
        bounds.windows(2).map(|pair| pair[0]..pair[1]).collect()
    }

    fn is_space(text: &str, range: &Range<usize>) -> bool {
        text[range.clone()].chars().all(char::is_whitespace)
    }

    /// Option-Left (Control-Left elsewhere): the start of the word before
    /// the caret, or of the text.
    pub fn word_start_before(text: &str, offset: usize) -> usize {
        segments(text)
            .into_iter()
            .rev()
            .find(|segment| segment.start < offset && !is_space(text, segment))
            .map_or(0, |segment| segment.start)
    }

    /// Option-Right on a Mac: the end of the word after the caret, or of
    /// the text.
    pub fn word_end_after(text: &str, offset: usize) -> usize {
        segments(text)
            .into_iter()
            .find(|segment| segment.end > offset && !is_space(text, segment))
            .map_or(text.len(), |segment| segment.end)
    }

    /// Control-Right on Windows and Linux: past the word the caret is in
    /// and the space after it, to the start of the next word.
    pub fn next_word_start(text: &str, offset: usize) -> usize {
        let segments = segments(text);
        let mut rest = segments
            .iter()
            .filter(|segment| segment.end > offset)
            .peekable();
        let mut position = offset;
        if let Some(word) = rest.next_if(|segment| !is_space(text, segment)) {
            position = word.end;
        }
        while let Some(space) = rest.next_if(|segment| is_space(text, segment)) {
            position = space.end;
        }
        position
    }

    /// The word Option-Right (Control-Right elsewhere) and Option-Delete go
    /// to on this platform.
    pub fn word_right(text: &str, offset: usize) -> usize {
        if cfg!(target_os = "macos") {
            word_end_after(text, offset)
        } else {
            next_word_start(text, offset)
        }
    }

    /// What a double-click on the character at `offset` selects: its word,
    /// run of space or mark; with `trailing_space` (Windows), a word with
    /// the space after it.
    pub fn word_at(text: &str, offset: usize, trailing_space: bool) -> Range<usize> {
        let segments = segments(text);
        let Some(index) = segments
            .iter()
            .position(|segment| segment.contains(&offset))
            .or(segments.len().checked_sub(1))
        else {
            return offset..offset;
        };
        let mut range = segments[index].clone();
        if trailing_space
            && !is_space(text, &range)
            && let Some(next) = segments.get(index + 1)
            && is_space(text, next)
            && !text[next.clone()].contains('\n')
        {
            range.end = next.end;
        }
        range
    }

    /// The start of the paragraph `offset` is in: just after the line
    /// break before it.
    pub fn paragraph_start(text: &str, offset: usize) -> usize {
        text[..offset].rfind('\n').map_or(0, |index| index + 1)
    }

    /// The end of the paragraph `offset` is in: at the line break after it.
    pub fn paragraph_end(text: &str, offset: usize) -> usize {
        text[offset..]
            .find('\n')
            .map_or(text.len(), |index| offset + index)
    }

    /// What a triple-click in text of several lines selects: the
    /// paragraph with its line break.
    pub fn paragraph_at(text: &str, offset: usize) -> Range<usize> {
        let end = paragraph_end(text, offset);
        let end = if end < text.len() { end + 1 } else { end };
        paragraph_start(text, offset)..end
    }

    /// Control-T: the range to replace, what replaces it and where the
    /// caret goes. The characters either side of the caret swap and the
    /// caret moves past them; at the end of a line, the two before it swap.
    pub fn transpose(text: &str, offset: usize) -> Option<(Range<usize>, String, usize)> {
        if offset == 0 {
            return None;
        }
        let at_end = offset == text.len() || text[offset..].starts_with('\n');
        let (first, second) = if at_end {
            let middle = previous_grapheme(text, offset);
            (previous_grapheme(text, middle)..middle, middle..offset)
        } else {
            let start = previous_grapheme(text, offset);
            (start..offset, offset..next_grapheme(text, offset))
        };
        if first.is_empty() || second.is_empty() {
            return None;
        }
        let (a, b) = (&text[first.clone()], &text[second.clone()]);
        if a.contains('\n') || b.contains('\n') {
            return None;
        }
        Some((first.start..second.end, format!("{b}{a}"), second.end))
    }

    /// Control-Delete on a Mac: where to delete back to so that only the
    /// last code point of the character before the caret goes (an accent
    /// typed as its own mark, say).
    pub fn decomposed_backward(text: &str, offset: usize) -> usize {
        let start = previous_grapheme(text, offset);
        text[start..offset]
            .char_indices()
            .next_back()
            .map_or(start, |(index, _)| start + index)
    }

    /// What a one-line field (`<input>`) keeps of pasted text: line breaks
    /// at the end dropped, the others made spaces.
    pub fn one_line(text: &str) -> String {
        text.trim_end_matches(['\r', '\n'])
            .replace("\r\n", " ")
            .replace(['\r', '\n'], " ")
    }

    /// Pasted text for a field of several lines, its line breaks all `\n`.
    pub fn normalize_breaks(text: &str) -> String {
        text.replace("\r\n", "\n").replace('\r', "\n")
    }

    /// The byte offset of a UTF-16 offset into `text`.
    pub fn utf8_offset(text: &str, utf16: usize) -> usize {
        let mut utf8 = 0;
        let mut counted = 0;
        for c in text.chars() {
            if counted >= utf16 {
                break;
            }
            counted += c.len_utf16();
            utf8 += c.len_utf8();
        }
        utf8
    }

    /// A field's text and selection at one moment.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Snapshot {
        pub text: String,
        pub selection: Range<usize>,
    }

    /// How many steps a field can take back.
    const DEPTH: usize = 200;

    /// An edit made by typing: characters put in, or deleted one at a time.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Typing {
        Insert,
        Delete,
    }

    /// A field's Undo and Redo, as Chromium's editing keeps them: a run of
    /// typed characters is one step, and so is a run of characters deleted
    /// one at a time with whatever is typed straight after it, until the
    /// caret is moved or another kind of edit comes; every other edit (a
    /// paste, a cut, deleting a word or a line) is a step of its own.
    /// Taking a step back selects the text it brings back.
    #[derive(Default, Debug)]
    pub struct History {
        undo: Vec<Snapshot>,
        redo: Vec<Snapshot>,
        typing: Option<Typing>,
    }

    impl History {
        /// Notes the state before an edit, and how it was made.
        pub fn record(&mut self, before: Snapshot, typing: Option<Typing>) {
            let continues = match (self.typing, typing) {
                (Some(_), Some(Typing::Insert)) => true,
                (Some(Typing::Delete), Some(Typing::Delete)) => true,
                _ => false,
            };
            if !continues {
                self.undo.push(before);
                if self.undo.len() > DEPTH {
                    self.undo.remove(0);
                }
            }
            self.redo.clear();
            self.typing = typing;
        }

        /// Ends the run of typing: what is typed next is a new step.
        pub fn close(&mut self) {
            self.typing = None;
        }

        pub fn clear(&mut self) {
            *self = Self::default();
        }

        #[cfg(test)]
        pub fn can_undo(&self) -> bool {
            !self.undo.is_empty()
        }

        /// The state before the last step, with what it brings back
        /// selected, given the state now (which Redo returns to).
        pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
            let before = self.undo.pop()?;
            self.typing = None;
            let selection = changed(&current.text, &before.text, current.selection.end);
            self.redo.push(current);
            Some(Snapshot {
                text: before.text,
                selection,
            })
        }

        /// The state the last Undo left, with the caret after what it
        /// changes.
        pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
            let after = self.redo.pop()?;
            self.typing = None;
            let end = changed(&current.text, &after.text, current.selection.start).end;
            self.undo.push(current);
            Some(Snapshot {
                text: after.text,
                selection: end..end,
            })
        }
    }

    /// The part of `to` that `from` does not have: what lies between their
    /// common start and their common end. The common start goes no further
    /// than `at`, where the caret in `from` shows the edit was, so that of
    /// two equal characters the one edited is the one found.
    pub fn changed(from: &str, to: &str, at: usize) -> Range<usize> {
        let mut prefix = 0;
        for (a, b) in from.chars().zip(to.chars()) {
            if a != b || prefix + a.len_utf8() > at {
                break;
            }
            prefix += a.len_utf8();
        }
        let suffix: usize = from[prefix..]
            .chars()
            .rev()
            .zip(to[prefix..].chars().rev())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.len_utf8())
            .sum();
        prefix..to.len() - suffix
    }
}

#[cfg(test)]
mod tests {
    use super::editing::{
        self, History, Snapshot, Typing, changed, decomposed_backward, next_grapheme,
        next_word_start, normalize_breaks, one_line, paragraph_at, paragraph_end, paragraph_start,
        previous_grapheme, transpose, word_at, word_end_after, word_start_before,
    };
    use super::hard_lines;

    #[test]
    fn line_breaks_split_the_text_into_lines_by_byte() {
        assert_eq!(hard_lines("one"), [0..3]);
        assert_eq!(hard_lines("a\nbc\n"), [0..1, 2..4, 5..5]);
        assert_eq!(hard_lines(""), [0..0]);
        assert_eq!(hard_lines("é\nx"), [0..2, 3..4]);
    }

    /// Byte offsets of the stops reached by pressing a key over and over,
    /// in UTF-16 units as the browser reports them.
    fn walk(text: &str, from: usize, step: fn(&str, usize) -> usize) -> Vec<usize> {
        let mut stops = Vec::new();
        let mut at = from;
        loop {
            let next = step(text, at);
            if next == at {
                break;
            }
            at = next;
            stops.push(text[..at].encode_utf16().count());
        }
        stops
    }

    // The stops below were read from the production app (Chromium on
    // macOS) pressing Option-Right and Option-Left in its search field.

    #[test]
    fn option_right_stops_where_chromium_does() {
        let text = "foo.bar-baz_qux it's  a/b c:d 3.14 x";
        assert_eq!(
            walk(text, 0, word_end_after),
            [3, 4, 7, 8, 15, 20, 23, 24, 25, 27, 28, 29, 34, 36]
        );
        let text = "example.com e.g. U.S.A. file_name.jpg 1,000.5 don't";
        assert_eq!(
            walk(text, 0, word_end_after),
            [
                7, 8, 11, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 33, 34, 37, 45, 51
            ]
        );
        let text = "  lead  trail  ";
        assert_eq!(walk(text, 0, word_end_after), [6, 13, 15]);
    }

    #[test]
    fn option_left_stops_where_chromium_does() {
        let text = "foo.bar-baz_qux it's  a/b c:d 3.14 x";
        assert_eq!(
            walk(text, text.len(), word_start_before),
            [35, 30, 28, 27, 26, 24, 23, 22, 16, 8, 7, 4, 3, 0]
        );
        let text =
            "a.1 1.a a:1 x;y x,y a·b a'b 'q' a’b 3:45 v1.2.3 IMG_0042.JPG #tag @me 50% $5 a&b";
        assert_eq!(
            walk(text, text.len(), word_start_before)[..30],
            [
                79, 78, 77, 75, 74, 72, 70, 67, 66, 62, 61, 57, 56, 48, 41, 38, 37, 36, 32, 30, 29,
                28, 24, 20, 18, 17, 16, 14, 13, 12
            ]
        );
        let text = "  lead  trail  ";
        assert_eq!(walk(text, text.len(), word_start_before), [8, 2, 0]);
    }

    #[test]
    fn emoji_accents_and_dashes_are_words_of_their_own() {
        let text = "naïve café — e\u{301}t\u{e9} 👍🏽 ok 👨‍👩‍👧 x";
        assert_eq!(
            walk(text, 0, word_end_after),
            [5, 10, 12, 17, 22, 25, 34, 36]
        );
        assert_eq!(
            walk(text, text.len(), word_start_before),
            [35, 26, 23, 18, 13, 11, 6, 0]
        );
    }

    #[test]
    fn chinese_japanese_and_thai_break_by_dictionary() {
        let text = "日本語のテキスト です ok";
        assert_eq!(walk(text, 0, word_end_after), [3, 4, 8, 11, 14]);
        assert_eq!(walk(text, text.len(), word_start_before), [12, 9, 4, 3, 0]);
        let text = "ทุกสองสัปดาห์ 中文分词测试 한국어 텍스트";
        assert_eq!(
            walk(text, 0, word_end_after),
            [3, 6, 13, 16, 18, 20, 24, 28]
        );
    }

    #[test]
    fn right_to_left_words_go_in_the_order_they_are_stored() {
        let text = "שלום עולם abc";
        assert_eq!(walk(text, 0, word_end_after), [4, 9, 13]);
        assert_eq!(walk(text, text.len(), word_start_before), [10, 5, 0]);
    }

    #[test]
    fn control_right_elsewhere_goes_to_the_next_word_start() {
        let text = "hello brave  new.world";
        assert_eq!(next_word_start(text, 0), 6);
        assert_eq!(next_word_start(text, 2), 6);
        assert_eq!(next_word_start(text, 5), 6);
        assert_eq!(next_word_start(text, 6), 13);
        assert_eq!(next_word_start(text, 13), 16);
        assert_eq!(next_word_start(text, 16), 17);
        assert_eq!(next_word_start(text, 17), text.len());
        assert_eq!(next_word_start(text, text.len()), text.len());
    }

    #[test]
    fn a_word_inside_or_at_its_edge_moves_to_that_edge() {
        let text = "hello brave new world";
        assert_eq!(word_start_before(text, 8), 6);
        assert_eq!(word_end_after(text, 8), 11);
        assert_eq!(word_start_before(text, 6), 0);
        assert_eq!(word_end_after(text, 11), 15);
        assert_eq!(word_start_before(text, 0), 0);
        assert_eq!(word_end_after(text, text.len()), text.len());
        assert_eq!(word_start_before("", 0), 0);
        assert_eq!(word_end_after("", 0), 0);
    }

    #[test]
    fn double_click_selects_the_word_space_or_mark_under_it() {
        let text = "visit example.com  now";
        assert_eq!(word_at(text, 7, false), 6..13);
        assert_eq!(word_at(text, 13, false), 13..14);
        assert_eq!(word_at(text, 17, false), 17..19);
        assert_eq!(word_at(text, 0, false), 0..5);
        assert_eq!(word_at(text, text.len(), false), 19..22);
        // Windows takes the space after a word with it.
        assert_eq!(word_at(text, 1, true), 0..6);
        assert_eq!(word_at(text, 15, true), 14..19);
        assert_eq!(word_at("", 0, false), 0..0);
        let emoji = "a 👍🏽 b";
        assert_eq!(word_at(emoji, 2, false), 2..10);
    }

    #[test]
    fn graphemes_keep_emoji_and_marks_whole() {
        let text = "e\u{301}👨‍👩‍👧x";
        assert_eq!(next_grapheme(text, 0), 3);
        assert_eq!(next_grapheme(text, 3), 3 + "👨‍👩‍👧".len());
        assert_eq!(previous_grapheme(text, text.len()), text.len() - 1);
        assert_eq!(previous_grapheme(text, text.len() - 1), 3);
        assert_eq!(previous_grapheme(text, 3), 0);
        assert_eq!(previous_grapheme(text, 0), 0);
        assert_eq!(next_grapheme(text, text.len()), text.len());
        // Control-Delete takes the accent off, not the letter.
        assert_eq!(decomposed_backward(text, 3), 1);
        assert_eq!(decomposed_backward("ab", 2), 1);
        assert_eq!(decomposed_backward("", 0), 0);
    }

    #[test]
    fn paragraphs_run_between_line_breaks() {
        let text = "one two\nthree\n\nfour";
        assert_eq!(paragraph_start(text, 5), 0);
        assert_eq!(paragraph_end(text, 5), 7);
        assert_eq!(paragraph_start(text, 10), 8);
        assert_eq!(paragraph_end(text, 8), 13);
        assert_eq!(paragraph_start(text, 14), 14);
        assert_eq!(paragraph_end(text, 14), 14);
        assert_eq!(paragraph_end(text, 16), text.len());
        assert_eq!(paragraph_at(text, 9), 8..14);
        assert_eq!(paragraph_at(text, 17), 15..19);
        assert_eq!(paragraph_at("single", 2), 0..6);
    }

    #[test]
    fn control_t_swaps_as_chromium_does() {
        // "hello br|ave" became "hello barve" with the caret after the pair.
        let text = "hello brave new world";
        assert_eq!(transpose(text, 8), Some((7..9, "ar".into(), 9)));
        assert_eq!(transpose(text, 5), Some((4..6, " o".into(), 6)));
        // At the end, the two before the caret.
        assert_eq!(transpose(text, 21), Some((19..21, "dl".into(), 21)));
        assert_eq!(transpose(text, 0), None);
        assert_eq!(transpose("a", 1), None);
        assert_eq!(transpose("ab\ncd", 3), None);
        assert_eq!(transpose("ab\ncd", 2), Some((0..2, "ba".into(), 2)));
        assert_eq!(transpose("é👍x", 2), Some((0..6, "👍é".into(), 6)));
    }

    #[test]
    fn pasting_into_one_line_does_what_input_does() {
        // Read from the production search field.
        assert_eq!(one_line("a\nb\r\nc\n\n"), "a b c");
        assert_eq!(one_line("\n\nfirst\nsecond"), "  first second");
        assert_eq!(one_line("plain"), "plain");
        assert_eq!(one_line("x\ry"), "x y");
        assert_eq!(normalize_breaks("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn utf16_offsets_become_bytes() {
        let text = "a👍é";
        assert_eq!(editing::utf8_offset(text, 0), 0);
        assert_eq!(editing::utf8_offset(text, 1), 1);
        assert_eq!(editing::utf8_offset(text, 3), 5);
        assert_eq!(editing::utf8_offset(text, 4), 7);
        assert_eq!(editing::utf8_offset(text, 9), 7);
    }

    fn state(text: &str, at: usize) -> Snapshot {
        Snapshot {
            text: text.into(),
            selection: at..at,
        }
    }

    fn type_into(history: &mut History, text: &mut String, typed: &str) {
        for c in typed.chars() {
            history.record(state(text, text.len()), Some(Typing::Insert));
            text.push(c);
        }
    }

    fn backspace(history: &mut History, text: &mut String, times: usize) {
        for _ in 0..times {
            history.record(state(text, text.len()), Some(Typing::Delete));
            text.pop();
        }
    }

    // The steps below were read from the production app's inspector
    // notes, a field the browser edits by itself.

    #[test]
    fn typing_is_one_step_and_deleting_then_typing_another() {
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "ab cd");
        backspace(&mut history, &mut text, 2);
        type_into(&mut history, &mut text, "xy");
        assert_eq!(text, "ab xy");
        let back = history.undo(state(&text, 5)).unwrap();
        assert_eq!(back.text, "ab cd");
        assert_eq!(back.selection, 3..5);
        let back = history.undo(back).unwrap();
        assert_eq!(back, state("", 0));
        assert!(!history.can_undo());
        let again = history.redo(back).unwrap();
        assert_eq!(again, state("ab cd", 5));
        let again = history.redo(again).unwrap();
        assert_eq!(again, state("ab xy", 5));
    }

    #[test]
    fn moving_the_caret_or_another_edit_starts_a_new_step() {
        // "ab xy", Left, "z", Option-Delete; then each Undo.
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "ab cd");
        backspace(&mut history, &mut text, 2);
        type_into(&mut history, &mut text, "xy");
        history.close();
        history.record(state("ab xy", 4), Some(Typing::Insert));
        text = "ab xzy".into();
        history.record(state(&text, 5), None);
        text = "ab y".into();

        let first = history.undo(state(&text, 3)).unwrap();
        assert_eq!(first.text, "ab xzy");
        assert_eq!(first.selection, 3..5);
        let second = history.undo(first).unwrap();
        assert_eq!(second, state("ab xy", 4));
        let third = history.undo(second).unwrap();
        assert_eq!(third.text, "ab cd");
        assert_eq!(third.selection, 3..5);
        let fourth = history.undo(third).unwrap();
        assert_eq!(fourth, state("", 0));

        let redone = history.redo(fourth).unwrap();
        assert_eq!(redone, state("ab cd", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab xy", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab xzy", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab y", 3));
        assert!(history.redo(redone).is_none());
    }

    #[test]
    fn deleting_after_typing_is_a_step_of_its_own() {
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "abc");
        backspace(&mut history, &mut text, 1);
        let back = history.undo(state(&text, 2)).unwrap();
        assert_eq!(back.text, "abc");
        assert_eq!(back.selection, 2..3);
    }

    #[test]
    fn an_edit_after_undo_drops_what_could_be_redone() {
        let mut history = History::default();
        history.record(state("", 0), None);
        let back = history.undo(state("pasted", 6)).unwrap();
        history.record(back, Some(Typing::Insert));
        assert!(history.redo(state("x", 1)).is_none());
        assert!(history.can_undo());
        history.clear();
        assert!(!history.can_undo());
        assert!(history.undo(state("x", 1)).is_none());
    }

    #[test]
    fn the_changed_part_leaves_out_what_both_share() {
        assert_eq!(changed("ab y", "ab xzy", 3), 3..5);
        assert_eq!(changed("ab xzy", "ab xy", 5), 4..4);
        assert_eq!(changed("ab xy", "ab cd", 5), 3..5);
        assert_eq!(changed("aaa", "aaaa", 3), 3..4);
        assert_eq!(changed("", "", 0), 0..0);
        assert_eq!(changed("é", "éé", 2), 2..4);
        assert_eq!(changed("x👍", "x", 5), 1..1);
        // Of two line breaks, the one deleted where the caret is.
        assert_eq!(changed("line\nend", "line\n\nend", 4), 4..5);
        assert_eq!(changed("aa", "aaa", 1), 1..2);
    }
}
