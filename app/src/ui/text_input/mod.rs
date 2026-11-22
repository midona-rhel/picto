// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! This module is the app's text field (`TextInput`). This file holds the
//! component, its actions and its public functions. `keys` binds the keys,
//! `commands` holds the editing commands, `mouse` handles the pointer,
//! `input_handler` answers the system's input method, `element` lays the
//! text out and draws it, and `editing` holds the editing rules that need
//! neither a layout nor GPUI.
mod commands;
/// Holds the editing rules of a text field, which are the rules that
/// Chromium's text fields follow on macOS, Windows and Linux. Its functions
/// work on byte offsets into the text and need no layout. They find the
/// edges of characters (graphemes), words and paragraphs, decide what a
/// one-line field keeps of pasted text, and keep the history for Undo and
/// Redo.
mod editing;
mod element;
mod input_handler;
mod keys;
mod mouse;

use std::ops::Range;

use editing::Typing;
use element::TextElement;
use gpui::{
    App, Bounds, Context, CursorStyle, FocusHandle, Focusable, FontWeight, KeyBinding, MouseButton,
    Pixels, Point, ShapedLine, SharedString, Window, actions, div, prelude::*, px,
};
pub use keys::bind;
use mouse::Granularity;
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
    /// The key of the record that the field's value belongs to. The view
    /// that owns the field chooses the key (`set_field`), and [`Committed`]
    /// carries it.
    field_key: Option<u64>,
    original: SharedString,
    kind: Kind,
    /// True when Enter emits [`Submitted`] and Escape is passed on to the
    /// view that holds the field.
    submits: bool,
    /// The number of pixels the text of a one-line field is drawn to the
    /// left of the element's left edge, so that the caret stays in view.
    scroll: Pixels,
    /// The time at which the caret last moved or the text last changed. The
    /// caret's blinking is counted from this time, so the caret is drawn
    /// during the first 500 ms after it.
    blink: std::time::Instant,
    blink_task: Option<gpui::Task<()>>,
    /// True while the field is an in-place rename that has not yet been
    /// committed or cancelled.
    renaming: bool,
    /// True when the field takes line breaks (Shift+Enter). While such a
    /// field has the keyboard focus and its text does not fit on one line,
    /// the text is drawn wrapped over as many rows as it needs.
    multiline: bool,
    /// True when the text needs more than one line, because it has a line
    /// break or is wider than the field.
    overflowing: bool,
    /// The lines that were drawn in the last frame, when the text was drawn
    /// wrapped: for each line, the byte offset of its first byte, its layout
    /// and its top edge in pixels below the top of the text. The value is
    /// `None` when the text was drawn on one line.
    last_lines: Option<Vec<(usize, gpui::WrappedLine, Pixels)>>,
    /// The scroll handle of the box that `ui::field` draws around the text
    /// of an expanded field.
    area: gpui::ScrollHandle,
    /// The caret offset for which that box was last scrolled to bring the
    /// caret into view.
    revealed: Option<usize>,
    /// The field's own history for Undo and Redo.
    history: editing::History,
    /// True when a caret at a place where a line wraps is drawn at the end
    /// of the upper row and not at the start of the lower row. Command-Right
    /// leaves the caret there.
    upstream: bool,
    /// The distance from the left edge of the text that Up and Down return
    /// to after they have passed through rows that are shorter.
    goal_x: Option<Pixels>,
    /// Whether the press that is being dragged selects by character, by word
    /// or by line.
    granularity: Granularity,
    /// The range that the press selected before the drag began. The
    /// selection always contains it.
    drag_origin: Range<usize>,
    /// The last position of the pointer while its button is held. The text
    /// is scrolled while this point is past the field's edge.
    drag_point: Option<Point<Pixels>>,
    autoscroll: Option<gpui::Task<()>>,
    /// True when the last edit was a Control-K. Another Control-K then adds
    /// to the kill buffer instead of replacing it.
    killing: bool,
    /// The typeface the field draws its text in, when it is not the one of
    /// its kind.
    face: Option<Face>,
    /// The name that assistive technology is given for the field.
    name: Option<SharedString>,
    /// True for a password field (see [`TextInput::masked`]).
    masked: bool,
}

/// The typeface a field draws its text in: the font family, the size, the
/// line height and the weight. The font viewer uses it for its sample
/// texts, which are edited in the font they show. A field with a face of its
/// own draws its caret in the colour of its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub family: SharedString,
    pub size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
}

impl TextInput {
    /// Returns an empty field of `kind` with no placeholder. The builder
    /// methods below set the rest: a placeholder, Enter that submits, a
    /// value that is committed when the field loses the keyboard focus,
    /// line breaks, and an in-place rename.
    pub fn new(kind: Kind, cx: &mut Context<Self>) -> Self {
        Self {
            // The Tab key can move the keyboard focus to a field.
            focus_handle: cx.focus_handle().tab_stop(true),
            content: "".into(),
            placeholder: "".into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            field_key: None,
            original: "".into(),
            kind,
            // A text area passes Escape on to the dialog that holds it.
            submits: kind == Kind::Area,
            scroll: px(0.),
            blink: std::time::Instant::now(),
            blink_task: None,
            renaming: false,
            multiline: kind == Kind::Area,
            overflowing: false,
            last_lines: None,
            area: gpui::ScrollHandle::new(),
            revealed: None,
            history: editing::History::default(),
            upstream: false,
            goal_x: None,
            granularity: Granularity::Character,
            drag_origin: 0..0,
            drag_point: None,
            autoscroll: None,
            killing: false,
            face: None,
            name: None,
            masked: false,
        }
    }

    /// Makes the field a password field. A bullet is drawn for each
    /// character, and the text itself is kept. Copy and Cut do nothing. A
    /// double-click selects the whole text, and the commands that move by a
    /// word go to the start or the end of the text, because the words cannot
    /// be seen. Assistive technology is given the bullets, and the input
    /// method is given no text.
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// Returns the text as it is drawn. For a password field that is one
    /// bullet for each character of the text.
    pub(super) fn drawn(&self) -> SharedString {
        if self.masked {
            SharedString::from(
                std::iter::repeat_n(BULLET, self.content.chars().count()).collect::<String>(),
            )
        } else {
            self.content.clone()
        }
    }

    /// Converts a byte offset in the text to the byte offset of the same
    /// place in the text as it is drawn.
    pub(super) fn drawn_offset(&self, offset: usize) -> usize {
        if !self.masked {
            return offset;
        }
        self.content[..offset.min(self.content.len())].chars().count() * BULLET.len_utf8()
    }

    /// Converts a byte offset in the text as it is drawn to the byte offset
    /// of the same place in the text.
    pub(super) fn content_offset(&self, drawn: usize) -> usize {
        if !self.masked {
            return drawn;
        }
        self.content
            .char_indices()
            .nth(drawn / BULLET.len_utf8())
            .map_or(self.content.len(), |(offset, _)| offset)
    }

    pub(super) fn is_masked(&self) -> bool {
        self.masked
    }

    /// Sets the name that assistive technology is given for the field. A
    /// field without a name is known by its placeholder.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Changes the field's accessible name (see [`TextInput::name`]).
    pub fn set_name(&mut self, name: impl Into<SharedString>) {
        self.name = Some(name.into());
    }

    /// Adds to `root` what assistive technology reads of the field: its
    /// role, its name, its text, its placeholder and, as a text run inside
    /// it, the selection or the caret. It returns `root` unchanged in a
    /// frame in which the window does not build its accessibility tree
    /// (`a11y::active`).
    fn describe(
        &self,
        root: gpui::Stateful<gpui::Div>,
        window: &Window,
    ) -> gpui::Stateful<gpui::Div> {
        if !crate::ui::a11y::active(window) {
            return root;
        }
        let role = match self.kind {
            _ if self.masked => gpui::Role::PasswordInput,
            Kind::Number => gpui::Role::SpinButton,
            Kind::Search | Kind::Sidebar | Kind::Popover | Kind::Menu => gpui::Role::SearchInput,
            _ if self.multiline => gpui::Role::MultilineTextInput,
            _ => gpui::Role::TextInput,
        };
        let mut root = root.role(role).aria_value(self.drawn());
        if !self.placeholder.is_empty() {
            root = root.aria_placeholder(crate::i18n::t_shared(self.placeholder.clone()));
        }
        if let Some(name) = &self.name {
            root = root.aria_label(name.clone());
        }
        if self.kind == Kind::Number
            && let Ok(number) = self.content.trim().parse::<f64>()
        {
            root = root.aria_numeric_value(number);
        }
        let text = self.drawn();
        let (start, end) = (
            self.drawn_offset(self.selected_range.start),
            self.drawn_offset(self.selected_range.end),
        );
        let (anchor, focus) = if self.selection_reversed {
            (end, start)
        } else {
            (start, end)
        };
        root.a11y_synthetic_children(move |builder| {
            let lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
            // Converts a byte offset to the index of the character in the
            // run.
            let index = |offset: usize| text[..offset.min(text.len())].chars().count();
            let mut run = gpui::accesskit::Node::new(gpui::Role::TextRun);
            run.set_value(text.to_string());
            run.set_character_lengths(lengths);
            let id = builder.synthetic_node_id("run");
            builder.push_child(id, run);
            let at = |offset| gpui::accesskit::TextPosition {
                node: id,
                character_index: index(offset),
            };
            builder
                .parent_node()
                .set_text_selection(gpui::accesskit::TextSelection {
                    anchor: at(anchor),
                    focus: at(focus),
                });
        })
    }

    /// Makes the field draw its text in `face` instead of the typeface of
    /// its kind.
    pub fn set_face(&mut self, face: Face, cx: &mut Context<Self>) {
        if self.face.as_ref() != Some(&face) {
            self.face = Some(face);
            cx.notify();
        }
    }

    /// Returns the field's line height. It is the line height of the
    /// field's face when the field has one, and else the one of its kind.
    pub(super) fn line_height(&self) -> f32 {
        self.face
            .as_ref()
            .map_or(self.kind.line_height(), |face| face.line_height)
    }

    /// Returns whether the caret is drawn in the colour of the text, which
    /// it is for a field with a face of its own. The caret of every other
    /// field is drawn in the theme's text colour.
    pub(super) fn caret_in_text_color(&self) -> bool {
        self.face.is_some()
    }

    /// Sets the text that the empty field shows. `placeholder` is the
    /// English text. It is translated into the language of the interface
    /// each time the field is drawn.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Makes Enter emit [`Submitted`], and makes the field pass Escape on to
    /// the view that holds it. The fields of dialogs are made this way.
    pub fn submits(mut self) -> Self {
        self.submits = true;
        self
    }

    /// Makes the field commit its value ([`Committed`]) when it loses the
    /// keyboard focus, as the inspector's fields do. An expanded field is
    /// then drawn on one line again.
    pub fn commits_on_blur(self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.on_blur(&self.focus_handle, window, |this, _, cx| {
            this.commit(cx);
            // The field is drawn again, so that an expanded field goes back
            // to one line.
            cx.notify();
        })
        .detach();
        self
    }

    /// Makes the field take line breaks with Shift+Enter. While the field
    /// has the keyboard focus and its text does not fit on one line,
    /// `ui::field` draws the field expanded.
    pub fn expanding(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Makes the field an in-place rename, as for a sidebar row or a
    /// subfolder tile of the grid. The field holds `value`, the current
    /// name, with all of it selected. Enter or losing the keyboard focus
    /// commits the rename and Escape cancels it ([`Renamed`]).
    pub fn renaming(mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        self.content = value.to_owned().into();
        self.original = self.content.clone();
        self.selected_range = 0..self.content.len();
        self.renaming = true;
        cx.on_blur(&self.focus_handle, window, |this, _, cx| {
            this.finish_rename(true, cx)
        })
        .detach();
        self
    }

    /// Ends an in-place rename and emits [`Renamed`]: the trimmed text when
    /// `commit` is true, and a cancel otherwise. It does nothing when the
    /// rename has already ended.
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
    pub fn select_everything(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        self.moved();
        cx.notify();
    }
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
    /// Empties the field and its history for Undo and Redo.
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
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }
    /// Puts the caret after the last character of the text.
    pub fn caret_at_end(&mut self, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    /// Replaces the text and empties the history for Undo and Redo. Escape
    /// goes back to this text.
    pub fn set_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.original = text.clone().into();
        self.content = text.into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.history.clear();
        self.moved();
        cx.notify();
    }
    /// Returns whether the field is a dialog's text area (`Kind::Area`). The
    /// text of a text area is always drawn wrapped from the top of its box,
    /// and Enter starts a new line in it.
    fn text_area(&self) -> bool {
        self.kind == Kind::Area
    }
    /// Returns whether the field has the keyboard focus and its text needs
    /// more than one line. `ui::field` then draws the field expanded over
    /// what is below it.
    pub fn expanded(&self, window: &Window) -> bool {
        self.multiline && self.overflowing && self.focus_handle.is_focused(window)
    }
    /// Returns the scroll handle of the box that `ui::field` draws around
    /// the text of an expanded field.
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

/// The character that a password field draws in place of each character of
/// its text.
const BULLET: char = '\u{2022}';

/// The line height of a dialog's text area in pixels, which is 1.4 times
/// its font size of 13px.
pub const AREA_LINE: f32 = 13. * 1.4;

/// Says which of the app's text fields a field is. The kind decides the
/// field's font, font size and line height. The frame around the field
/// (fill, border, padding) is drawn by `ui::field`.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The search field of the toolbar.
    Search,
    /// The filter field above the sidebar's tree.
    Sidebar,
    /// The inspector's fields, such as the name and the notes, and the
    /// address editor of the source list.
    Inspector,
    /// A form field in the interface font at 13px: the fields of dialogs,
    /// the search fields of the Settings window, and the rename of a
    /// subfolder tile in the grid.
    Form,
    /// A number field. Its text is 12px and is drawn against the right
    /// edge.
    Number,
    /// The field in which a sidebar row is renamed in place. It has the
    /// row's own font and font size, on a line that fits the 20px field.
    SidebarRename,
    /// The search field at the top of the library switcher.
    Popover,
    /// The search row of a context menu, and the search fields of the tag
    /// and folder pickers and dialogs.
    Menu,
    /// A small field at 12px on a 24px line, such as the range fields of
    /// the filter bar.
    Compact,
    /// The field for a colour's hex code, in the monospaced font at 12px.
    Mono,
    /// A dialog's text area. Its text is 13px on a line 1.4 times as tall,
    /// and is drawn wrapped from the top of the box. Enter starts a new
    /// line, and Escape is passed on to the dialog.
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
    /// Returns the colour of the placeholder. It is the theme's tertiary
    /// text colour, except in the search field of the library switcher,
    /// where it is `Theme::placeholder`.
    fn placeholder(self, theme: crate::theme::Theme) -> gpui::Hsla {
        match self {
            Kind::Popover => theme.placeholder().into(),
            _ => theme.text_tertiary().into(),
        }
    }
}

/// The event a field emits when its value is committed. It carries the key
/// the value was set for (`TextInput::set_field`) and the new value.
pub struct Committed(pub u64, pub String);
impl gpui::EventEmitter<Committed> for TextInput {}
/// The event a field made with [`TextInput::submits`] emits when Enter is
/// pressed in it.
pub struct Submitted;
impl gpui::EventEmitter<Submitted> for TextInput {}
/// The event a field emits when Backspace is pressed while it is empty.
pub struct EmptyBackspace;
impl gpui::EventEmitter<EmptyBackspace> for TextInput {}

/// The event a field emits when an in-place rename ends. Enter or losing
/// the keyboard focus commits the trimmed text, and Escape cancels the
/// rename.
pub enum Renamed {
    Commit(String),
    Cancel,
}
impl gpui::EventEmitter<Renamed> for TextInput {}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // An expanded field is as tall as its wrapped text. Any other field
        // shows one line, which is centred in the height of the field.
        let root = if self.expanded(window) {
            div().w_full()
        } else if self.text_area() {
            // A text area fills its box and draws its text from the top, so
            // that a press below the text still reaches the field.
            div().size_full()
        } else {
            div().size_full().flex().items_center()
        };
        #[cfg(test)]
        {
            let name = format!("field \"{}\"", self.placeholder);
            crate::ui::focus::names::name_text(&self.focus_handle(cx), name, cx);
        }
        self.describe(root.id("text-input"), window)
            .key_context(if self.multiline {
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
            .on_action(
                cx.listener(|this, _: &SelectLineStart, _, cx| this.line_edge(false, true, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| this.line_edge(true, true, cx)))
            .on_action(
                cx.listener(|this, _: &ParagraphStart, _, cx| {
                    this.paragraph_edge(false, false, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ParagraphEnd, _, cx| this.paragraph_edge(true, false, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectParagraphStart, _, cx| {
                this.paragraph_edge(false, true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectParagraphEnd, _, cx| {
                this.paragraph_edge(true, true, cx)
            }))
            .on_action(cx.listener(|this, _: &ParagraphBackward, _, cx| {
                this.paragraph_step(false, false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ParagraphForward, _, cx| {
                    this.paragraph_step(true, false, cx)
                }),
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
                if this.text_area() {
                    this.edit(this.selected_range.clone(), "\n", Some(Typing::Insert), cx);
                    return;
                }
                if this.submits {
                    cx.emit(Submitted);
                    return;
                }
                this.commit(cx);
                // An in-place rename is ended here and not when the field
                // loses the keyboard focus, because a window that is not
                // active never reports that loss.
                this.finish_rename(true, cx);
                window.blur(cx);
            }))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                // The dialog or screen that holds a field made with
                // `submits` decides what Escape does.
                if this.submits {
                    cx.propagate();
                    return;
                }
                // Escape empties the sidebar's filter field, and the field
                // keeps the keyboard focus.
                if this.kind == Kind::Sidebar {
                    if !this.content.is_empty() {
                        this.content = "".into();
                        this.selected_range = 0..0;
                        cx.notify();
                    }
                    return;
                }
                this.finish_rename(false, cx);
                // A field with a stored value goes back to that value.
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
            .map(|root| match &self.face {
                Some(face) => root
                    .font_family(face.family.clone())
                    .line_height(px(face.line_height))
                    .text_size(px(face.size))
                    .font_weight(face.weight),
                None => root
                    .font_family(self.kind.font())
                    .line_height(px(self.kind.line_height()))
                    .text_size(px(self.kind.size())),
            })
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
