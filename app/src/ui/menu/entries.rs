//! Defines what a menu is made of: its entries (rows, submenus, rows of
//! swatches, views and separators), and the functions that an owner builds
//! the entries with.
use gpui::*;

pub enum Entry {
    Item(Item),
    Separator,
    /// A row that opens its own rows beside the menu. Submenus do not nest.
    Submenu(Submenu),
    /// A row of colour swatches.
    Swatches(Swatches),
    /// A view of the owner's that is drawn in the menu in place of a row,
    /// such as the icon picker in "Change Icon". The view handles the
    /// pointer itself. The arrow keys do not stop on it and the search does
    /// not find it.
    View(ViewEntry),
}

pub struct ViewEntry {
    pub(super) view: AnyView,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) padded: bool,
}

/// Returns an entry that shows `view` in the menu at this width and height,
/// for content that is not a row.
pub fn view(view: impl Into<AnyView>, width: f32, height: f32) -> ViewEntry {
    ViewEntry {
        view: view.into(),
        width,
        height,
        padded: false,
    }
}

impl ViewEntry {
    /// Gives the entry 6px of padding at its left and at its right, which
    /// suits a view that is a panel of controls.
    pub fn padded(mut self) -> Self {
        self.padded = true;
        self
    }
}

impl From<ViewEntry> for Entry {
    fn from(view: ViewEntry) -> Self {
        Entry::View(view)
    }
}

pub struct Item {
    /// The action that the menu emits to its owner when the row is chosen.
    pub(super) action: &'static str,
    pub(super) label: SharedString,
    /// Other words that the menu's search finds this row by.
    pub(super) keywords: Vec<SharedString>,
    pub(super) icon: Option<&'static str>,
    /// A picture that is drawn in place of the icon, such as an
    /// application's icon in "Open With Other".
    pub(super) image: Option<std::sync::Arc<Image>>,
    pub(super) shortcut: Option<SharedString>,
    pub(super) danger: bool,
    pub(super) disabled: bool,
    /// `Some` gives the row the space for a check mark, and `Some(true)`
    /// draws the check mark in it.
    pub(super) checked: Option<bool>,
    pub(super) excluded: bool,
    pub(super) keep_open: bool,
    /// The value that tells this row apart from other rows with the same
    /// action. See [`Menu::chosen_value`].
    pub(super) value: Option<SharedString>,
    /// The action that the menu emits to its owner when the row is clicked
    /// with the right button.
    pub(super) context_action: Option<&'static str>,
}

/// Returns a row that emits `action` when it is chosen. Its label is
/// translated into the interface's language here.
pub fn item(action: &'static str, label: impl Into<SharedString>) -> Item {
    Item {
        action,
        label: crate::i18n::t_shared(label.into()),
        keywords: Vec::new(),
        icon: None,
        image: None,
        shortcut: None,
        danger: false,
        disabled: false,
        checked: None,
        excluded: false,
        keep_open: false,
        value: None,
        context_action: None,
    }
}

impl Item {
    /// Sets the label to `label` as it is written, without translating it.
    /// The rows of the application menu use this. Their titles are in
    /// English in every language.
    pub fn english(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Sets a picture that is drawn 16px square in place of the icon.
    pub fn image(mut self, image: std::sync::Arc<Image>) -> Self {
        self.image = Some(image);
        self
    }
    /// Sets the shortcut that the row shows, written the way the platform
    /// writes it, for example `⌘⌫`.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
    /// Makes the row show the keys that the shortcut `id` is bound to now
    /// (`keymap::hint`), so that the row shows the new keys after the user
    /// changes the shortcut.
    pub fn shortcut_for(mut self, id: &str, cx: &App) -> Self {
        self.shortcut = crate::keymap::hint(id, cx);
        self
    }
    /// Marks the row as the destructive one of its menu. It is drawn in the
    /// negative colour.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// Gives the row the space for a check mark, and draws the mark when
    /// `checked` is true. The owner changes it while the menu is open with
    /// `Menu::set_checked`.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
    /// Marks the row as a value that a filter leaves out. The row is drawn
    /// in the negative colour, with its label struck through and a cross in
    /// place of the check mark.
    pub fn excluded(mut self, excluded: bool) -> Self {
        self.excluded = excluded;
        self
    }
    /// Makes the menu stay open when the row is chosen. The owner is still
    /// told, so the user can change several rows that have check marks
    /// without opening the menu again.
    pub fn keep_open(mut self, keep_open: bool) -> Self {
        self.keep_open = keep_open;
        self
    }
    pub fn keywords<S: Into<SharedString>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }
    /// Sets the action that the menu emits to its owner, together with the
    /// row's value, when the row is clicked with the right button. The menu
    /// stays open.
    pub fn context_action(mut self, action: &'static str) -> Self {
        self.context_action = Some(action);
        self
    }
}

impl From<Item> for Entry {
    fn from(item: Item) -> Self {
        Entry::Item(item)
    }
}

pub struct Submenu {
    pub(super) label: SharedString,
    pub(super) icon: Option<&'static str>,
    pub(super) children: Vec<Entry>,
}

/// Returns a row that opens `children` as a submenu. Its label is translated
/// here, as [`item`]'s is.
pub fn submenu(label: impl Into<SharedString>, children: Vec<Entry>) -> Submenu {
    Submenu {
        label: crate::i18n::t_shared(label.into()),
        icon: None,
        children,
    }
}

impl Submenu {
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl From<Submenu> for Entry {
    fn from(submenu: Submenu) -> Self {
        Entry::Submenu(submenu)
    }
}

/// One colour in a row of swatches.
pub struct Swatch {
    /// The text of the swatch's tooltip.
    pub label: SharedString,
    /// The swatch's colour. `None` is the default colour, which is drawn in
    /// the primary text colour.
    pub color: Option<Rgba>,
    /// The value that [`Menu::chosen_value`] returns when this swatch is
    /// picked.
    pub value: SharedString,
}

pub struct Swatches {
    pub(super) action: &'static str,
    pub(super) swatches: Vec<Swatch>,
    pub(super) selected: Option<usize>,
}

/// Returns a row of colour swatches. When the user picks one, the menu emits
/// `action` to its owner with the swatch's value, and stays open with a ring
/// drawn around the new choice.
pub fn swatches(action: &'static str, swatches: Vec<Swatch>, selected: Option<usize>) -> Swatches {
    Swatches {
        action,
        swatches,
        selected,
    }
}

impl From<Swatches> for Entry {
    fn from(swatches: Swatches) -> Self {
        Entry::Swatches(swatches)
    }
}
