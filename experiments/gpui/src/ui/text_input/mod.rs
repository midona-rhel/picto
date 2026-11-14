// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! A text field: the component, its actions and its public API. The keys
//! are in `keys`, the editing commands in `commands`, the pointer in
//! `mouse`, the input method in `input_handler`, layout and painting in
//! `element`, and the editing rules with no layout or GPUI in `editing`.
mod commands;
/// The editing rules of the browser's text fields on a Mac (and their
/// Windows and Linux equivalents), on byte offsets into the text and with
/// no layout: graphemes, words as Chromium finds them, paragraphs, what a
/// one-line field keeps of pasted text, and the history Undo walks.
mod editing;
mod element;
mod input_handler;
mod keys;
mod mouse;

use std::ops::Range;

use editing::Typing;
use element::TextElement;
use gpui::{
    App, Bounds, Context, CursorStyle, FocusHandle, Focusable, KeyBinding, MouseButton, Pixels,
    Point, ShapedLine, SharedString, Window, actions, div, prelude::*, px,
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

impl TextInput {
    /// A field of `kind`, empty and with no placeholder. The builder
    /// methods below give it the rest: a placeholder, Enter that submits,
    /// a value committed when it loses focus, line breaks, an in-place
    /// rename.
    pub fn new(kind: Kind, cx: &mut Context<Self>) -> Self {
        Self {
            // A field is a stop for Tab, as an `<input>` is.
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
            // A text area leaves Escape to the dialog that holds it.
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
        }
    }

    /// What the empty field shows.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Enter submits the field ([`Submitted`]) and Escape is left to what
    /// holds it: dialog fields, the Tags screen's search, the source list's
    /// address editor.
    pub fn submits(mut self) -> Self {
        self.submits = true;
        self
    }

    /// A stored field (the inspector's): losing focus commits its value
    /// ([`Committed`]) and takes an expanded field back to its one line.
    pub fn commits_on_blur(self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.on_blur(&self.focus_handle, window, |this, _, cx| {
            this.commit(cx);
            // An expanded field goes back to its one line.
            cx.notify();
        })
        .detach();
        self
    }

    /// The field takes line breaks with Shift+Enter and, while focused
    /// text overflows its line, is drawn expanded by its frame
    /// (`InspectorField`).
    pub fn expanding(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// An in-place rename (a sidebar row's, a grid subfolder tile's),
    /// holding the current name selected. Enter or losing focus commits it
    /// and Escape cancels it ([`Renamed`]).
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
    pub fn select_everything(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
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
    /// A dialog's text area (`GlassTextarea`): its text is always drawn
    /// wrapped from the top of its box, and Enter is a line break.
    fn text_area(&self) -> bool {
        self.kind == Kind::Area
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

/// `.textarea`'s `line-height: 1.4` at 13px.
pub const AREA_LINE: f32 = 13. * 1.4;

/// Which of the app's text fields this is, which decides its type. The frame
/// around it (fill, border, padding) is `ui::field`.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// The toolbar search.
    Search,
    /// The sidebar's tree filter.
    Sidebar,
    /// The inspector's name and notes, and the source list's address
    /// editor.
    Inspector,
    /// A form field at the UI font's 13px: dialog fields, the settings
    /// searches, a grid subfolder tile's rename (`FolderRenameInput`).
    Form,
    /// A number, at 12px and against the right edge (`CompactNumberInput`).
    Number,
    /// A sidebar row renamed in place (`Sidebar.module.css` `.renameInput`):
    /// the row's own type, on a line that fits the 20px field.
    SidebarRename,
    /// The search at the top of a popover (`LibrarySwitcherPopover`).
    Popover,
    /// A context menu's search row (`.searchInput`), and the search of the
    /// Add Tags and Folders dialogs.
    Menu,
    /// A small field at 12px on a 26px line: the filter bar's ranges.
    Compact,
    /// A colour's hex code, in the monospaced font at 12px (`.hexField`).
    Mono,
    /// A dialog's text area (`GlassInput.module.css` `.textarea`): 13px on
    /// a 1.4 line, wrapped from the top of its box, where Enter starts a
    /// new line and Escape is left to the dialog.
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
/// Enter was pressed in a field made with [`TextInput::submits`].
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

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Expanded, the text is as tall as it wraps; otherwise one line,
        // centred in the field.
        let root = if self.expanded(window) {
            div().w_full()
        } else if self.text_area() {
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
            if this.text_area() {
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
