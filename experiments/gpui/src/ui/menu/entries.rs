//! What a menu is made of: entries (rows, submenus, swatch rows, views and
//! separators) and the builders owners describe their menus with.
use gpui::*;

pub enum Entry {
    Item(Item),
    Separator,
    /// A row that opens its own rows beside the menu. Submenus do not nest.
    Submenu(Submenu),
    /// A row of colour swatches (`ColorPicker`).
    Swatches(Swatches),
    /// A view of the owner's drawn in place of rows, as a menu entry with
    /// `custom` content is (the icon picker in "Change Icon"). It handles
    /// its own pointer; the arrow keys and the search pass over it.
    View(ViewEntry),
}

pub struct ViewEntry {
    pub(super) view: AnyView,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) padded: bool,
}

/// A view shown in the menu at this size, for content that is not rows.
pub fn view(view: impl Into<AnyView>, width: f32, height: f32) -> ViewEntry {
    ViewEntry {
        view: view.into(),
        width,
        height,
        padded: false,
    }
}

impl ViewEntry {
    /// Inside `.customItem`'s 6px either side, as a panel of controls is.
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
    /// What the owner is told was chosen.
    pub(super) action: &'static str,
    pub(super) label: SharedString,
    /// Other words the search row matches.
    pub(super) keywords: Vec<SharedString>,
    pub(super) icon: Option<&'static str>,
    pub(super) shortcut: Option<SharedString>,
    pub(super) danger: bool,
    pub(super) disabled: bool,
    /// `Some` reserves the check slot; `Some(true)` shows the check.
    pub(super) checked: Option<bool>,
    pub(super) excluded: bool,
    pub(super) keep_open: bool,
    /// Tells rows that share an action apart; see [`Menu::chosen_value`].
    pub(super) value: Option<SharedString>,
    /// What a right click on the row tells the owner (`contextAction`).
    pub(super) context_action: Option<&'static str>,
}

pub fn item(action: &'static str, label: impl Into<SharedString>) -> Item {
    Item {
        action,
        label: label.into(),
        keywords: Vec::new(),
        icon: None,
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
    /// What [`Menu::chosen_value`] gives when this row is chosen, for rows
    /// that share an action, such as one row per group.
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    /// The shortcut as the platform writes it, e.g. `⌘⌫`.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
    /// The hint for a registry shortcut as it is bound now (`shortcutHint`),
    /// so the row follows a rebinding.
    pub fn shortcut_for(mut self, id: &str, cx: &App) -> Self {
        self.shortcut = crate::keymap::hint(id, cx);
        self
    }
    /// The one destructive row of a menu, drawn in the negative colour.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    /// A row with a check slot, checked or not. The owner changes it while
    /// the menu is open with `Menu::set_checked`.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
    /// A value a filter leaves out: negative colour, struck through, with a
    /// cross in the check slot.
    pub fn excluded(mut self, excluded: bool) -> Self {
        self.excluded = excluded;
        self
    }
    /// Choosing the row tells the owner and leaves the menu open, so several
    /// toggles can be changed together.
    pub fn keep_open(mut self, keep_open: bool) -> Self {
        self.keep_open = keep_open;
        self
    }
    pub fn keywords<S: Into<SharedString>>(mut self, words: impl IntoIterator<Item = S>) -> Self {
        self.keywords = words.into_iter().map(Into::into).collect();
        self
    }
    /// A right click on the row tells the owner this action, with the
    /// row's value, and leaves the menu open.
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

pub fn submenu(label: impl Into<SharedString>, children: Vec<Entry>) -> Submenu {
    Submenu {
        label: label.into(),
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

/// One colour of a swatch row.
pub struct Swatch {
    /// Its tooltip.
    pub label: SharedString,
    /// `None` is the default colour, drawn in the primary text colour.
    pub color: Option<Rgba>,
    /// What [`Menu::chosen_value`] gives when it is picked.
    pub value: SharedString,
}

pub struct Swatches {
    pub(super) action: &'static str,
    pub(super) swatches: Vec<Swatch>,
    pub(super) selected: Option<usize>,
}

/// A row of colour swatches. Picking one tells the owner, with the swatch's
/// value, and leaves the menu open with the new choice ringed.
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
