// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill,
    point, prelude::*, px, relative, size,
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
    ]
);

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
}

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

/// Where byte `offset` is drawn in wrapped lines, from the text's top left.
fn position_in(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    offset: usize,
    line_height: Pixels,
) -> Option<(Pixels, Pixels)> {
    let (start, line, top) = lines.iter().rev().find(|(start, _, _)| *start <= offset)?;
    let local = line.position_for_index((offset - start).min(line.len()), line_height)?;
    Some((local.x, *top + local.y))
}

/// The byte nearest a point in wrapped lines, measured from the text's top
/// left.
fn index_in(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    position: Point<Pixels>,
    line_height: Pixels,
) -> usize {
    let Some((start, line, top)) = lines
        .iter()
        .rev()
        .find(|(_, _, top)| *top <= position.y)
        .or(lines.first())
    else {
        return 0;
    };
    let local = point(position.x.max(px(0.)), (position.y - *top).max(px(0.)));
    let index = match line.closest_index_for_position(local, line_height) {
        Ok(index) | Err(index) => index,
    };
    start + index.min(line.len())
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
    /// A name edited in place, in the UI font at the size of the text it
    /// replaces.
    Rename(f32),
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
}

impl Kind {
    fn font(self) -> &'static str {
        match self {
            Kind::Search
            | Kind::Form
            | Kind::Number
            | Kind::Popover
            | Kind::Rename(_)
            | Kind::Menu
            | Kind::Compact => crate::theme::FONT_UI,
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
            | Kind::Menu => crate::theme::TEXT_MD,
            Kind::Rename(size) => size,
        }
    }
    fn line_height(self) -> f32 {
        match self {
            Kind::Search => 22.,
            Kind::Sidebar => 26.,
            Kind::Inspector => 20.,
            Kind::Form | Kind::Number => self.size() + 2.,
            Kind::Rename(size) => size + 2.,
            Kind::SidebarRename => crate::theme::SIDEBAR_RENAME_HEIGHT - 2.,
            Kind::Popover => 48.,
            Kind::Menu => 36.,
            Kind::Compact => 24.,
            Kind::Mono => 30.,
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

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            cx.emit(EmptyBackspace);
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        _window.focus(&self.focus_handle, cx);
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline {
                text.replace("\r\n", "\n").replace('\r', "\n")
            } else {
                text.replace("\n", " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    /// Shift+Enter in a multi-line field: a line break at the caret.
    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    /// The caret one line up or down in wrapped text, at the same distance
    /// from the left; past the first or last line, to the start or end.
    fn vertical(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(lines) = self.last_lines.as_ref() else {
            let end = if down { self.content.len() } else { 0 };
            self.move_to(end, cx);
            return;
        };
        let line_height = px(self.kind.line_height());
        let cursor = self.cursor_offset();
        let Some((x, y)) = position_in(lines, cursor, line_height) else {
            return;
        };
        let target = if down {
            y + line_height * 1.5
        } else {
            y - line_height * 0.5
        };
        let total = lines
            .last()
            .map(|(_, line, top)| *top + line.size(line_height).height)
            .unwrap_or_default();
        let offset = if target < px(0.) {
            0
        } else if target >= total {
            self.content.len()
        } else {
            index_in(lines, point(x, target), line_height)
        };
        self.move_to(offset, cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.blink = std::time::Instant::now();
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        if let (Some(bounds), Some(lines)) = (self.last_bounds.as_ref(), self.last_lines.as_ref()) {
            let line_height = px(self.kind.line_height());
            let local = point(position.x - bounds.left(), position.y - bounds.top());
            return index_in(lines, local, line_height);
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.blink = std::time::Instant::now();
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
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
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
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

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        self.blink = std::time::Instant::now();
        cx.notify();
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

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

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
            let (start_x, top) = position_in(lines, range.start, line_height)?;
            let (end_x, end_top) = position_in(lines, range.end, line_height)?;
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
        if let Some(lines) = self.last_lines.as_ref() {
            let local = gpui::point(point.x - bounds.left(), point.y - bounds.top());
            let index = index_in(lines, local, px(self.kind.line_height()));
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
        input.multiline && input.overflowing && input.focus_handle.is_focused(window)
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
        let rows = lines
            .iter()
            .map(|(_, line, _)| line.wrap_boundaries().len() + 1)
            .sum::<usize>();
        let overflowing = rows > 1;
        let glyphs = font_size * 1.2;
        let caret = |offset: usize| {
            position_in(&lines, offset, line_height)
                .map(|(x, y)| point(bounds.left() + x, bounds.top() + y))
        };
        let (cursor, selection) = if selected_range.is_empty() {
            let quad = caret(cursor).map(|at| {
                fill(
                    Bounds::new(
                        point(at.x, at.y + (line_height - glyphs) / 2.),
                        size(px(1.), glyphs),
                    ),
                    theme.text(),
                )
            });
            (quad, Vec::new())
        } else {
            let mut quads = Vec::new();
            if let (Some(start), Some(end)) =
                (caret(selected_range.start), caret(selected_range.end))
            {
                let row = |from: Pixels, to: Pixels, top: Pixels| {
                    fill(
                        Bounds::from_corners(point(from, top), point(to, top + line_height)),
                        theme.selection(),
                    )
                };
                if start.y == end.y {
                    quads.push(row(start.x, end.x, start.y));
                } else {
                    quads.push(row(start.x, bounds.right(), start.y));
                    let mut top = start.y + line_height;
                    while top < end.y {
                        quads.push(row(bounds.left(), bounds.right(), top));
                        top += line_height;
                    }
                    quads.push(row(bounds.left(), end.x, end.y));
                }
            }
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
                let caret = cursor.take();
                if let Some(cursor) = caret.clone().filter(|_| caret_on) {
                    window.paint_quad(cursor);
                }
                (None, Some((std::mem::take(lines), caret)))
            }
        };

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
                        let top = caret.bounds.top() - px(6.);
                        let bottom = caret.bounds.bottom() + px(6.);
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
        } else {
            div().size_full().flex().items_center()
        };
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
        .on_action(cx.listener(|this, _: &Commit, window, cx| {
            if this.submits {
                cx.emit(Submitted);
                return;
            }
            this.commit(cx);
            window.blur(cx);
        }))
        .on_action(cx.listener(|this, _: &Cancel, window, cx| {
            // A dialog or screen decides what Escape does in its field.
            if this.submits {
                cx.propagate();
                return;
            }
            this.finish_rename(false, cx);
            // The sidebar filter empties on Escape; a stored field goes
            // back to its saved value.
            this.content = if this.kind == Kind::Sidebar {
                "".into()
            } else {
                this.original.clone()
            };
            this.selected_range = 0..0;
            window.blur(cx);
            cx.notify();
        }))
        .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
        .on_mouse_move(cx.listener(Self::on_mouse_move))
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
            focus_handle: cx.focus_handle(),
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
        cx.notify();
    }
    /// The caret after the last character, where a browser leaves it in a
    /// field given its value before it takes focus.
    pub fn caret_to_end(&mut self, cx: &mut Context<Self>) {
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
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
    /// Replaces the text, which Escape then goes back to.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.original = text.clone().into();
        self.content = text.into();
        self.selected_range = 0..0;
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
    /// A name edited in place: it commits when it loses focus, as the
    /// inspector's fields do.
    pub fn rename(size: f32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::field("", window, cx);
        input.kind = Kind::Rename(size);
        input
    }
    /// Sets the size of a name edited in place, to match the text it
    /// replaces.
    pub fn set_rename_size(&mut self, size: f32, cx: &mut Context<Self>) {
        if matches!(self.kind, Kind::Rename(_)) && self.kind != Kind::Rename(size) {
            self.kind = Kind::Rename(size);
            cx.notify();
        }
    }
    pub fn set_field(&mut self, key: u64, value: String, cx: &mut Context<Self>) {
        if self.field_key == Some(key) && self.content != self.original {
            return;
        }
        self.field_key = Some(key);
        self.original = value.clone().into();
        self.content = value.into();
        self.selected_range = 0..0;
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
pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Commit, Some("TextInput")),
        KeyBinding::new("shift-enter", Newline, Some("TextArea")),
        KeyBinding::new("up", Up, Some("TextArea")),
        KeyBinding::new("down", Down, Some("TextArea")),
        KeyBinding::new("escape", Cancel, Some("TextInput")),
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("TextInput")),
        KeyBinding::new("cmd-v", Paste, Some("TextInput")),
        KeyBinding::new("cmd-c", Copy, Some("TextInput")),
        KeyBinding::new("cmd-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
    ]);
}

#[cfg(test)]
mod tests {
    use super::hard_lines;

    #[test]
    fn line_breaks_split_the_text_into_lines_by_byte() {
        assert_eq!(hard_lines("one"), [0..3]);
        assert_eq!(hard_lines("a\nbc\n"), [0..1, 2..4, 5..5]);
        assert_eq!(hard_lines(""), [0..0]);
        assert_eq!(hard_lines("é\nx"), [0..2, 3..4]);
    }
}
