// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
#![allow(unused_imports)]
use std::ops::Range;

use gpui::{
    App, Application, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyBinding, Keystroke,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, WindowBounds,
    WindowOptions, actions, black, div, fill, hsla, opaque_grey, point, prelude::*, px, relative,
    rgb, rgba, size, white, yellow,
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
    /// How far the text is scrolled left to keep the caret in view.
    scroll: Pixels,
    /// When the caret last moved; it shows for 500 ms from then, and blinks.
    blink: std::time::Instant,
    blink_task: Option<gpui::Task<()>>,
    /// An in-place rename that has not yet been committed or cancelled.
    renaming: bool,
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
    /// A sidebar row renamed in place (`Sidebar.module.css` `.renameInput`):
    /// the row's own type, on a line that fits the 20px field.
    Rename,
    /// The search at the top of a popover (`LibrarySwitcherPopover`).
    Popover,
}

impl Kind {
    fn font(self) -> &'static str {
        match self {
            Kind::Search | Kind::Popover => crate::theme::FONT_UI,
            Kind::Sidebar | Kind::Inspector | Kind::Rename => crate::theme::FONT_INSPECTOR,
        }
    }
    fn size(self) -> f32 {
        match self {
            Kind::Search => crate::theme::TEXT_SM,
            Kind::Sidebar | Kind::Inspector | Kind::Popover | Kind::Rename => crate::theme::TEXT_MD,
        }
    }
    fn line_height(self) -> f32 {
        match self {
            Kind::Search => 22.,
            Kind::Sidebar => 26.,
            Kind::Inspector => 20.,
            Kind::Rename => crate::theme::SIDEBAR_RENAME_HEIGHT - 2.,
            Kind::Popover => 48.,
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

const BLINK: std::time::Duration = std::time::Duration::from_millis(500);
/// A stored field's new value, with the key it was set for.
pub struct Committed(pub u64, pub String);
impl gpui::EventEmitter<Committed> for TextInput {}

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
            self.replace_text_in_range(None, &text.replace("\n", " "), window, cx);
        }
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
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
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
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;

        assert_eq!(last_layout.text, self.content);
        let utf8_index = last_layout.index_for_x(point.x - line_point.x + self.scroll)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
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
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
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
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let focused = input.focus_handle.is_focused(window);
        let marked_range = input.marked_range.clone();
        let placeholder = input.placeholder.clone();
        let kind = input.kind;
        let previous_scroll = input.scroll;
        let style = window.text_style();
        let theme = crate::theme::theme(cx);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let shape = |text: SharedString, color: gpui::Hsla, window: &mut Window| {
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = match marked_range.as_ref() {
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
            };
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
        let scroll = if !focused || line.width <= caret_room {
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
        PrepaintState {
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
        let line = prepaint.line.take().unwrap();
        let (selection, cursor) = (prepaint.selection.take(), prepaint.cursor.take());
        let origin = point(bounds.left() - prepaint.scroll, bounds.top());
        let line_height = window.line_height();
        // The caret is on for the first half of each second since it moved.
        let since_move = self.input.read(cx).blink.elapsed();
        let caret_on = (since_move.as_millis() / BLINK.as_millis()) % 2 == 0;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = selection {
                window.paint_quad(selection)
            }
            line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx)
                .unwrap();
            if let Some(cursor) = cursor.filter(|_| caret_on) {
                window.paint_quad(cursor);
            }
        });

        let focused = focus_handle.is_focused(window);
        self.input.update(cx, |input, cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            // Repaint at the caret's next change for as long as it shows.
            if focused && input.blink_task.is_none() {
                let wait = BLINK.as_millis() - since_move.as_millis() % BLINK.as_millis();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .key_context("TextInput")
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
            .on_action(cx.listener(|this, _: &Commit, window, cx| {
                this.commit(cx);
                window.blur(cx);
            }))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
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
            scroll: px(0.),
            blink: std::time::Instant::now(),
            blink_task: None,
            renaming: false,
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
    pub fn rename(value: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = "".into();
        input.kind = Kind::Rename;
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
    pub fn select_everything(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
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
    pub fn field(placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.placeholder = placeholder.into();
        input.kind = Kind::Inspector;
        cx.on_blur(&input.focus_handle, window, |this, _, cx| this.commit(cx))
            .detach();
        input
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
