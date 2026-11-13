//! The context menu (`ContextMenu.tsx`): a glass panel of rows at the
//! pointer, with check marks, submenus and a search row.
//!
//! A menu is shown one of two ways, and `presentation()` alone chooses:
//! drawn over the owner's window and kept 12px inside it, or with each panel
//! (the menu and its open submenu) in a pop-up window of its own, kept 12px
//! inside the display, so a menu can reach past the window's edge. In both,
//! the keyboard stays with the owner's window, where the menu's layer takes
//! it, and a press anywhere outside the panels closes the menu.
use crate::theme::Text as _;
use crate::ui::text_input::{self, TextInput};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::time::{Duration, Instant};

pub(crate) mod host;
pub use host::{Host, Return};

actions!(menu, [Up, Down, Confirm, Cancel, OpenSubmenu, CloseSubmenu]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", Up, Some("Menu")),
        KeyBinding::new("down", Down, Some("Menu")),
        KeyBinding::new("enter", Confirm, Some("Menu")),
        KeyBinding::new("escape", Cancel, Some("Menu")),
        KeyBinding::new("right", OpenSubmenu, Some("Menu")),
        KeyBinding::new("left", CloseSubmenu, Some("Menu")),
    ]);
}

/// How menus are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// Panels drawn over the owner's window, kept inside it.
    InWindow,
    /// Each panel in a pop-up window of its own, kept inside the display.
    PopUpWindows,
}

/// The one switch between the two presentations. Pop-up windows on macOS,
/// where GPUI opens a `WindowKind::PopUp` as a non-activating panel that can
/// be shown without taking the keyboard; the in-window overlay elsewhere,
/// and on macOS too with `PICTO_GPUI_MENU=window`. A menu whose pop-up
/// window cannot be opened falls back to the overlay by itself.
pub fn presentation() -> Presentation {
    let forced_in_window = std::env::var("PICTO_GPUI_MENU").is_ok_and(|value| value == "window");
    if cfg!(target_os = "macos") && !forced_in_window {
        Presentation::PopUpWindows
    } else {
        Presentation::InWindow
    }
}

/// The pop-up windows menus have open, and the pop-up windows a control
/// inside one of them opened (a select's list), each with the menu window it
/// was opened from. A child closes with its menu window, and while a child
/// has the keyboard its menu counts as having it.
#[derive(Default)]
struct PopUps {
    menus: Vec<AnyWindowHandle>,
    children: Vec<(AnyWindowHandle, AnyWindowHandle)>,
}

impl Global for PopUps {}

/// Whether this window is one of a menu's pop-up windows, where a control
/// cannot open anything over the window and must open a pop-up of its own
/// ([`adopt`]).
pub fn is_pop_up(window: &Window, cx: &App) -> bool {
    let handle = window.window_handle();
    cx.try_global::<PopUps>()
        .is_some_and(|pop_ups| pop_ups.menus.contains(&handle))
}

/// Records a pop-up window opened from a menu's pop-up window, so that it
/// closes with it.
pub fn adopt(owner: AnyWindowHandle, child: AnyWindowHandle, cx: &mut App) {
    let pop_ups = cx.default_global::<PopUps>();
    if pop_ups.menus.contains(&owner) {
        pop_ups.children.push((owner, child));
    } else {
        // The menu closed while the child opened.
        close_window(child, cx);
    }
}

/// Forgets a child pop-up its control closed itself.
pub fn release(child: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>()
        .children
        .retain(|(_, open)| *open != child);
}

fn register(window: AnyWindowHandle, cx: &mut App) {
    cx.default_global::<PopUps>().menus.push(window);
}

/// Forgets a menu window that is closing, and closes its children.
fn forget(window: AnyWindowHandle, cx: &mut App) {
    let pop_ups = cx.default_global::<PopUps>();
    pop_ups.menus.retain(|open| *open != window);
    let mut children = Vec::new();
    pop_ups.children.retain(|(owner, child)| {
        let keep = *owner != window;
        if !keep {
            children.push(*child);
        }
        keep
    });
    for child in children {
        let _ = child.update(cx, |_, window, _| window.remove_window());
    }
}

fn children_of(owners: &[AnyWindowHandle], cx: &App) -> Vec<AnyWindowHandle> {
    cx.try_global::<PopUps>().map_or(Vec::new(), |pop_ups| {
        pop_ups
            .children
            .iter()
            .filter(|(owner, _)| owners.contains(owner))
            .map(|(_, child)| *child)
            .collect()
    })
}

const WIDTH: f32 = 270.;
/// The panel keeps this far from the window's (or the display's) edges.
const MARGIN: f32 = 12.;
const PADDING: f32 = 4.;
const GAP: f32 = 2.;
/// A separator's line and its 2px margins.
const SEPARATOR: f32 = 5.;
/// `.searchArea`, and the input's left edge: 10px padding, the 16px glyph
/// and a 6px gap.
const SEARCH_HEIGHT: f32 = 36.;
const SEARCH_INPUT_LEFT: f32 = 32.;
/// `SEARCH_ITEM_THRESHOLD`: rows (items and submenus, separators aside) a
/// menu needs before it offers the search row.
const SEARCH_THRESHOLD: usize = 7;
/// `.empty`: 8px above and below one line of 12px text.
const EMPTY_HEIGHT: f32 = 30.;
/// `max-height: calc(100vh - 40px)`.
const VIEWPORT_INSET: f32 = 40.;
/// A submenu opens this far right of its menu and this far above its row.
const SUBMENU_GAP: f32 = 4.;
const SUBMENU_LIFT: f32 = 3.;
/// A row's horizontal padding, the icon slot with its gap, the check slot,
/// and the space before a shortcut.
const ROW_PADDING: f32 = 6.;
const ICON_SLOT: f32 = 22.;
const CHECK_SLOT: f32 = 16.;
const SHORTCUT_GAP: f32 = 28.;
/// Room around a pop-up panel for its drawn shadow (`--shadow-panel`
/// reaches 24px out and 36px down).
const SHADOW_ROOM: f32 = 40.;
/// After a window of the menu loses the keyboard, how long to wait for
/// another of its windows to report that it took it.
const ACTIVATION_SETTLE: Duration = Duration::from_millis(50);

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
    view: AnyView,
    width: f32,
    height: f32,
    padded: bool,
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
    action: &'static str,
    label: SharedString,
    /// Other words the search row matches.
    keywords: Vec<SharedString>,
    icon: Option<&'static str>,
    shortcut: Option<SharedString>,
    danger: bool,
    disabled: bool,
    /// `Some` reserves the check slot; `Some(true)` shows the check.
    checked: Option<bool>,
    excluded: bool,
    keep_open: bool,
    /// Tells rows that share an action apart; see [`Menu::chosen_value`].
    value: Option<SharedString>,
    /// What a right click on the row tells the owner (`contextAction`).
    context_action: Option<&'static str>,
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
    label: SharedString,
    icon: Option<&'static str>,
    children: Vec<Entry>,
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
    action: &'static str,
    swatches: Vec<Swatch>,
    selected: Option<usize>,
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

/// `.customItem`: `padding: 0 6px`.
const CUSTOM_PADDING: f32 = 6.;

/// `ColorPicker`'s row: 18px swatches with 4px above and below.
const SWATCH_ROW: f32 = 26.;
const SWATCH: f32 = 18.;

pub enum Event {
    /// A row was chosen. Unless it keeps the menu open, the menu is already
    /// closing.
    Chosen(&'static str),
}

/// Where an entry is: a row of the menu, or a row of one of its submenus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Path {
    top: usize,
    child: Option<usize>,
}

/// One line of a panel as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Entry(Path),
    Separator,
    /// A swatch row: not a stop for the arrow keys, nor found by a search.
    Swatches(Path),
    /// A view entry and its height in pixels.
    View(Path, u32),
}

/// A rectangle in the coordinates of the window or display it is placed in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn outset(&self, by: f32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2. * by,
            h: self.h + 2. * by,
        }
    }
    fn intersect(&self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect {
            x,
            y,
            w: (self.right().min(other.right()) - x).max(0.),
            h: (self.bottom().min(other.bottom()) - y).max(0.),
        }
    }
    fn from_bounds(bounds: Bounds<Pixels>) -> Rect {
        Rect {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
        }
    }
    fn to_bounds(self) -> Bounds<Pixels> {
        Bounds::new(point(px(self.x), px(self.y)), size(px(self.w), px(self.h)))
    }
}

/// `placeMenu`: the panel's corner at the pointer, moved inward so the
/// panel keeps the margin from every edge of the area. The height is the
/// panel's full height, before any maximum.
fn place(pointer: (f32, f32), w: f32, h: f32, area: Rect) -> (f32, f32) {
    let (mut x, mut y) = pointer;
    if x + w > area.right() - MARGIN {
        x = area.right() - w - MARGIN;
    }
    if y + h > area.bottom() - MARGIN {
        y = area.bottom() - h - MARGIN;
    }
    (x.max(area.x + MARGIN), y.max(area.y + MARGIN))
}

/// `placeMenu`'s `transform-origin` for the panel `place` puts there: the
/// corner at the pointer, or the right or bottom edge where the panel was
/// moved inward from that edge. Fractions of the width and height.
fn origin(pointer: (f32, f32), w: f32, h: f32, area: Rect) -> (f32, f32) {
    (
        if pointer.0 + w > area.right() - MARGIN {
            1.
        } else {
            0.
        },
        if pointer.1 + h > area.bottom() - MARGIN {
            1.
        } else {
            0.
        },
    )
}

/// `SubmenuPanel`'s placement: 4px right of the parent panel and 3px above
/// its row, on the parent's left instead when it would cross the margin on
/// the right, then moved up or down to keep the margin.
fn place_submenu(parent: Rect, row_top: f32, w: f32, h: f32, area: Rect) -> (f32, f32) {
    let mut x = parent.right() + SUBMENU_GAP;
    let mut y = row_top - SUBMENU_LIFT;
    if x + w > area.right() - MARGIN {
        x = parent.x - w - SUBMENU_GAP;
    }
    if y + h > area.bottom() - MARGIN {
        y = area.bottom() - h - MARGIN;
    }
    (x, y.max(area.y + MARGIN))
}

/// `cleanSeparators`: no separator first, last or twice in a row. A
/// submenu's rows leave out nested submenus, which are not drawn.
fn clean(entries: &[Entry], parent: Option<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let path = match parent {
            Some(top) => Path {
                top,
                child: Some(index),
            },
            None => Path {
                top: index,
                child: None,
            },
        };
        match entry {
            Entry::Separator => {
                if rows.last().is_some_and(|row| *row != Row::Separator) {
                    rows.push(Row::Separator);
                }
            }
            Entry::Submenu(_) if parent.is_some() => {}
            Entry::Swatches(_) => rows.push(Row::Swatches(path)),
            Entry::View(view) => rows.push(Row::View(path, view.height.round() as u32)),
            _ => rows.push(Row::Entry(path)),
        }
    }
    if rows.last() == Some(&Row::Separator) {
        rows.pop();
    }
    rows
}

/// `menuSearchScore`: a label that starts with the query, then one that
/// contains it, then the label and keywords containing it, then the query's
/// letters in order with the fewest gaps. Lower is better.
fn score(text: &str, label: &str, query: &str) -> Option<usize> {
    if label.starts_with(query) {
        return Some(0);
    }
    if let Some(at) = label.find(query) {
        return Some(10 + label[..at].chars().count());
    }
    if let Some(at) = text.find(query) {
        return Some(100 + text[..at].chars().count());
    }
    let query = query.chars().collect::<Vec<_>>();
    let (mut found, mut gap) = (0, 0);
    for letter in text.chars() {
        if found == query.len() {
            break;
        }
        if letter == query[found] {
            found += 1;
        } else if found > 0 {
            gap += 1;
        }
    }
    (found == query.len()).then_some(200 + gap)
}

/// `searchMenuEntries`: every enabled item and submenu whose label or
/// keywords match, best first and in menu order among equals. Submenu rows
/// are searched too, so a match inside one is a row that can be chosen
/// directly.
fn search(entries: &[Entry], raw: &str) -> Vec<Path> {
    let query = raw.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let matching = |label: &SharedString, keywords: &[SharedString]| {
        let label = label.to_lowercase();
        let text = format!(
            "{label} {}",
            keywords
                .iter()
                .map(|word| word.as_ref())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_lowercase();
        score(&text, &label, &query)
    };
    let mut matches = Vec::new();
    let mut order = 0;
    let mut visit = |entry: &Entry, path: Path| {
        let found = match entry {
            Entry::Separator | Entry::Swatches(_) | Entry::View(_) => return,
            Entry::Item(item) if item.disabled => None,
            Entry::Item(item) => matching(&item.label, &item.keywords),
            Entry::Submenu(submenu) => matching(&submenu.label, &[]),
        };
        if let Some(score) = found {
            matches.push((score, order, path));
        }
        order += 1;
    };
    for (top, entry) in entries.iter().enumerate() {
        visit(entry, Path { top, child: None });
        if let Entry::Submenu(submenu) = entry {
            for (child, entry) in submenu.children.iter().enumerate() {
                // A submenu inside a submenu has no row to show it.
                if !matches!(entry, Entry::Submenu(_)) {
                    visit(
                        entry,
                        Path {
                            top,
                            child: Some(child),
                        },
                    );
                }
            }
        }
    }
    matches.sort_by_key(|(score, order, _)| (*score, *order));
    matches.into_iter().map(|(_, _, path)| path).collect()
}

/// The row the arrow keys reach next among the rows that can be stopped
/// on, wrapping at the ends.
fn step(stops: &[bool], current: Option<usize>, forward: bool) -> Option<usize> {
    let rows = (0..stops.len())
        .filter(|index| stops[*index])
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return current;
    }
    let at = current.and_then(|current| rows.iter().position(|row| *row == current));
    let next = match (at, forward) {
        (Some(at), true) => (at + 1) % rows.len(),
        (Some(at), false) => (at + rows.len() - 1) % rows.len(),
        (None, true) => 0,
        (None, false) => rows.len() - 1,
    };
    Some(rows[next])
}

fn row_height(row: &Row) -> f32 {
    match row {
        Row::Entry(_) => theme::CONTROL_SM,
        Row::Separator => SEPARATOR,
        Row::Swatches(_) => SWATCH_ROW,
        Row::View(_, height) => *height as f32,
    }
}

/// `.items`: its padding, rows and gaps, or the empty-result line.
fn items_height(rows: &[Row], empty_line: bool) -> f32 {
    if rows.is_empty() {
        return 2. * PADDING + if empty_line { EMPTY_HEIGHT } else { 0. };
    }
    2. * PADDING + rows.iter().map(row_height).sum::<f32>() + GAP * (rows.len() - 1) as f32
}

/// Where a row starts below the top of its panel's items area.
fn row_offset(rows: &[Row], index: usize) -> f32 {
    PADDING
        + rows[..index.min(rows.len())]
            .iter()
            .map(|row| row_height(row) + GAP)
            .sum::<f32>()
}

/// The display a pop-up menu is shown on, and its root panel's window.
struct Screen {
    /// The display's usable area, in the display's coordinates.
    area: Rect,
    /// Where the owner window's content starts on that display.
    origin: (f32, f32),
    display: DisplayId,
    /// The part of the display the root panel's window covers. It is sized
    /// for the tallest the panel can become while searching.
    bounds: Rect,
    window: Option<AnyWindowHandle>,
}

/// The open submenu.
struct Opened {
    /// Its row in the menu as drawn, and its entry.
    row: usize,
    top: usize,
    /// The row under the pointer or reached with the arrow keys. While set,
    /// the arrow keys move in the submenu.
    current: Option<usize>,
    /// The panel, placed when it opened, in the menu's coordinates.
    rect: Rect,
    /// Its pop-up window, and the part of the display that covers.
    window: Option<AnyWindowHandle>,
    bounds: Rect,
    serial: u64,
}

/// What a pop-up window being opened will show.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    Submenu(u64),
}

pub struct Menu {
    entries: Vec<Entry>,
    position: Point<Pixels>,
    width: f32,
    search: Option<Entity<TextInput>>,
    query: String,
    /// The row under the pointer or reached with the arrow keys: one
    /// highlight for both, as in the Electron menu.
    current: Option<usize>,
    highlights: motion::Fades,
    submenu: Option<Opened>,
    submenu_highlights: motion::Fades,
    submenu_grace: Option<Task<()>>,
    serial: u64,
    scroll: ScrollHandle,
    focus: FocusHandle,
    /// When the root panel was first drawn, where it shows: it grows in
    /// from then (`menuIn`).
    shown: Option<Instant>,
    /// Set once dismissed: the menu shrinks and fades for 80 ms (`menuOut`)
    /// and takes no more input.
    closing: Option<Instant>,
    /// The value of the row or swatch last chosen.
    chosen: Option<SharedString>,
    presentation: Presentation,
    /// The window the owner draws the menu in.
    main: Option<AnyWindowHandle>,
    /// The owner window's size, the area an in-window menu is kept inside.
    viewport: Rect,
    screen: Option<Screen>,
    opening: Option<Role>,
    /// Windows whose activation is watched, so that the menu closes when
    /// none of its windows has the keyboard.
    watched: Vec<AnyWindowHandle>,
    activation: Option<Task<()>>,
    close_windows: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Event> for Menu {}

impl Focusable for Menu {
    /// The search field while the menu has one, else the menu itself.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.search {
            Some(search) => search.read(cx).focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl Menu {
    pub fn new(entries: Vec<Entry>, position: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        let searchable = entries
            .iter()
            .filter(|entry| !matches!(entry, Entry::Separator | Entry::View(_)))
            .count();
        let search = (searchable >= SEARCH_THRESHOLD).then(|| cx.new(TextInput::menu_search));
        let mut subscriptions = Vec::new();
        if let Some(search) = &search {
            subscriptions.push(cx.observe(search, |this, search, cx| {
                let query = search.read(cx).value();
                if query != this.query {
                    this.query = query;
                    this.current = None;
                    this.highlights = motion::Fades::default();
                    this.scroll.set_offset(point(px(0.), px(0.)));
                    this.close_submenu(cx);
                    cx.notify();
                }
            }));
        }
        Self {
            entries,
            position,
            width: WIDTH,
            search,
            query: String::new(),
            current: None,
            highlights: motion::Fades::default(),
            submenu: None,
            submenu_highlights: motion::Fades::default(),
            submenu_grace: None,
            serial: 0,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            shown: None,
            closing: None,
            chosen: None,
            presentation: presentation(),
            main: None,
            viewport: Rect::default(),
            screen: None,
            opening: None,
            watched: Vec::new(),
            activation: None,
            close_windows: None,
            _subscriptions: subscriptions,
        }
    }

    /// Whether the menu may offer its search row (`showSearch`); it does
    /// when it has at least seven rows.
    pub fn search(mut self, show: bool) -> Self {
        if !show {
            self.search = None;
        }
        self
    }

    /// Draws the menu in its owner's window rather than in pop-up windows,
    /// for views whose fields need the keyboard and whose calendars open
    /// over the window. (A select in a pop-up menu opens its list as a
    /// pop-up of its own, so selects alone do not need this.)
    pub fn in_window(mut self) -> Self {
        self.presentation = Presentation::InWindow;
        self
    }

    /// A width other than the usual 270px, for content that needs it.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Checks or unchecks the row with this action while the menu is open.
    pub fn set_checked(&mut self, action: &'static str, checked: bool, cx: &mut Context<Self>) {
        fn visit(entries: &mut [Entry], action: &str, checked: bool) {
            for entry in entries {
                match entry {
                    Entry::Item(item) if item.action == action => item.checked = Some(checked),
                    Entry::Submenu(submenu) => visit(&mut submenu.children, action, checked),
                    _ => {}
                }
            }
        }
        visit(&mut self.entries, action, checked);
        cx.notify();
    }

    /// Replaces the rows while the menu is open, for a menu whose rows
    /// follow what they change. The search, and the row highlighted by
    /// position, stay.
    pub fn set_entries(&mut self, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.close_submenu(cx);
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        self.closing = Some(Instant::now());
        self.submenu_grace = None;
        if self.presentation == Presentation::PopUpWindows {
            // The pop-up windows fade with the menu, then close.
            self.close_windows = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(motion::MENU_OUT).await;
                let _ = this.update(cx, |this, cx| this.close_pop_ups(cx));
            }));
        }
        cx.notify();
    }

    /// The value of the row or swatch last chosen, for rows that share an
    /// action ([`Item::value`], [`Swatch::value`]).
    pub fn chosen_value(&self) -> Option<SharedString> {
        self.chosen.clone()
    }

    /// Whether the keyboard is in the menu, its search or a field of an
    /// owner's row.
    pub fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx) || self.focus_handle(cx).is_focused(window)
    }

    /// Whether the menu has faded out and can be dropped.
    pub fn finished(&self) -> bool {
        self.closing
            .is_some_and(|since| since.elapsed() >= motion::MENU_OUT)
    }

    /// The root panel's scale and opacity now: growing in once it is first
    /// drawn, shrinking and fading once dismissed. Submenus have neither
    /// (`.submenuMenu { animation: none }`): an open one stays as it is until
    /// the menu is gone.
    fn root_motion(&self) -> (f32, f32) {
        match (self.closing, self.shown) {
            (Some(since), _) => motion::menu_out(since.elapsed()),
            (None, Some(shown)) => (motion::menu_in(shown.elapsed()), 1.),
            (None, None) => (motion::MENU_IN_SCALE, 1.),
        }
    }

    /// Whether the root panel is still growing in.
    fn entering(&self) -> bool {
        self.closing.is_none()
            && self
                .shown
                .is_none_or(|shown| shown.elapsed() < motion::MENU_IN)
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// The menu's rows as drawn: all of them, or the search's results.
    fn rows(&self) -> Vec<Row> {
        if self.searching() {
            search(&self.entries, &self.query)
                .into_iter()
                .map(Row::Entry)
                .collect()
        } else {
            clean(&self.entries, None)
        }
    }

    fn submenu_rows(&self, top: usize) -> Vec<Row> {
        match &self.entries[top] {
            Entry::Submenu(submenu) => clean(&submenu.children, Some(top)),
            _ => Vec::new(),
        }
    }

    fn entry(&self, path: Path) -> &Entry {
        match (&self.entries[path.top], path.child) {
            (Entry::Submenu(submenu), Some(child)) => &submenu.children[child],
            (entry, _) => entry,
        }
    }

    /// Whether the arrow keys stop on a row: an enabled item or a submenu.
    fn stops(&self, rows: &[Row]) -> Vec<bool> {
        rows.iter()
            .map(|row| match row {
                Row::Entry(path) => match self.entry(*path) {
                    Entry::Item(item) => !item.disabled,
                    Entry::Submenu(_) => true,
                    Entry::Separator | Entry::Swatches(_) | Entry::View(_) => false,
                },
                Row::Separator | Row::Swatches(_) | Row::View(..) => false,
            })
            .collect()
    }

    fn has_icons(&self, rows: &[Row]) -> bool {
        rows.iter().any(|row| match row {
            Row::Entry(path) => match self.entry(*path) {
                Entry::Item(item) => item.icon.is_some(),
                Entry::Submenu(submenu) => submenu.icon.is_some(),
                Entry::Separator | Entry::Swatches(_) | Entry::View(_) => false,
            },
            Row::Separator | Row::Swatches(_) | Row::View(..) => false,
        })
    }

    fn search_height(&self) -> f32 {
        if self.search.is_some() {
            SEARCH_HEIGHT
        } else {
            0.
        }
    }

    /// The panel's full height for these rows.
    fn height(&self, rows: &[Row]) -> f32 {
        self.search_height() + items_height(rows, self.searching())
    }

    /// The tallest the panel can be: every row, or every row a search can
    /// find, whichever is more.
    fn tallest(&self) -> f32 {
        let all = self.rows();
        let mut found = 0;
        for entry in &self.entries {
            match entry {
                Entry::Separator | Entry::Swatches(_) | Entry::View(_) => {}
                Entry::Item(_) => found += 1,
                Entry::Submenu(submenu) => {
                    found += 1 + submenu
                        .children
                        .iter()
                        .filter(|child| matches!(child, Entry::Item(_)))
                        .count();
                }
            }
        }
        let found = vec![
            Row::Entry(Path {
                top: 0,
                child: None
            });
            found
        ];
        self.height(&all)
            .max(self.height(&found))
            .max(self.search_height() + items_height(&[], true))
    }

    /// The area the menu is kept inside, and the pointer in its
    /// coordinates.
    fn area(&self) -> (Rect, (f32, f32)) {
        let pointer = (f32::from(self.position.x), f32::from(self.position.y));
        match (&self.screen, self.presentation) {
            (Some(screen), Presentation::PopUpWindows) => (
                screen.area,
                (screen.origin.0 + pointer.0, screen.origin.1 + pointer.1),
            ),
            _ => (self.viewport, pointer),
        }
    }

    /// The menu's panel: placed by its full height, drawn no taller than the
    /// area less 40px.
    fn root_rect(&self, rows: &[Row]) -> Rect {
        let (area, pointer) = self.area();
        let height = self.height(rows);
        let (x, y) = place(pointer, self.width, height, area);
        Rect {
            x,
            y,
            w: self.width,
            h: height.min(area.h - VIEWPORT_INSET),
        }
    }

    /// The `transform-origin` of the root panel for these rows.
    fn root_origin(&self, rows: &[Row]) -> (f32, f32) {
        let (area, pointer) = self.area();
        origin(pointer, self.width, self.height(rows), area)
    }

    /// The open submenu's row, entry and current row.
    fn opened(&self) -> Option<(usize, usize, Option<usize>)> {
        self.submenu
            .as_ref()
            .map(|open| (open.row, open.top, open.current))
    }

    fn set_current(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        if self.current == row {
            return;
        }
        if let Some(previous) = self.current {
            self.highlights
                .set(("row", previous), false, motion::MENU_ROW);
        }
        if let Some(next) = row {
            self.highlights.set(("row", next), true, motion::MENU_ROW);
        }
        self.current = row;
        cx.notify();
    }

    fn set_submenu_current(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        let Some(open) = &mut self.submenu else {
            return;
        };
        if open.current == row {
            return;
        }
        if let Some(previous) = open.current {
            self.submenu_highlights
                .set(("row", previous), false, motion::MENU_ROW);
        }
        if let Some(next) = row {
            self.submenu_highlights
                .set(("row", next), true, motion::MENU_ROW);
        }
        open.current = row;
        cx.notify();
    }

    fn arrow(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some((_, top, Some(current))) = self.opened() {
            let rows = self.submenu_rows(top);
            let next = step(&self.stops(&rows), Some(current), forward);
            self.set_submenu_current(next, cx);
            return;
        }
        let rows = self.rows();
        let next = step(&self.stops(&rows), self.current, forward);
        if self
            .submenu
            .as_ref()
            .is_some_and(|open| Some(open.row) != next)
        {
            self.close_submenu(cx);
        }
        self.set_current(next, cx);
    }

    /// Right arrow: into the open submenu, or opens the current row's.
    /// Returns whether it did either.
    fn right(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        if let Some((row, top, inside)) = self.opened()
            && row == current
        {
            if inside.is_none() {
                let first = step(&self.stops(&self.submenu_rows(top)), None, true);
                self.set_submenu_current(first, cx);
            }
            return true;
        }
        let rows = self.rows();
        match rows.get(current) {
            Some(Row::Entry(path)) if matches!(self.entry(*path), Entry::Submenu(_)) => {
                self.open_submenu(current, true, window, cx);
                true
            }
            _ => false,
        }
    }

    /// Left arrow: closes the open submenu. Returns whether one was open.
    fn left(&mut self, cx: &mut Context<Self>) -> bool {
        if self.submenu.is_some() {
            self.close_submenu(cx);
            true
        } else {
            false
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((_, top, Some(current))) = self.opened() {
            if let Some(Row::Entry(path)) = self.submenu_rows(top).get(current).copied() {
                self.choose(path, cx);
            }
            return;
        }
        let Some(current) = self.current else {
            return;
        };
        let rows = self.rows();
        match rows.get(current) {
            Some(Row::Entry(path)) if matches!(self.entry(*path), Entry::Submenu(_)) => {
                self.toggle_submenu(current, true, window, cx)
            }
            Some(Row::Entry(path)) => self.choose(*path, cx),
            _ => {}
        }
    }

    fn choose(&mut self, path: Path, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        let Entry::Item(item) = self.entry(path) else {
            return;
        };
        if item.disabled {
            return;
        }
        let (action, keep_open) = (item.action, item.keep_open);
        self.chosen = item.value.clone();
        cx.emit(Event::Chosen(action));
        if !keep_open {
            self.dismiss(cx);
        }
    }

    /// A right click on a row with a context action: the owner is told,
    /// and the menu stays open.
    fn choose_context(&mut self, path: Path, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        let Entry::Item(item) = self.entry(path) else {
            return;
        };
        if item.disabled {
            return;
        }
        if let Some(action) = item.context_action {
            self.chosen = item.value.clone();
            cx.emit(Event::Chosen(action));
            cx.notify();
        }
    }

    fn toggle_submenu(
        &mut self,
        row: usize,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.submenu.as_ref().is_some_and(|open| open.row == row) {
            self.close_submenu(cx);
        } else {
            self.open_submenu(row, keyboard, window, cx);
        }
    }

    /// The pointer moved onto a row that has no submenu: an open submenu
    /// closes after a grace period, unless the pointer reaches it first.
    fn leave_submenu(&mut self, cx: &mut Context<Self>) {
        if self.submenu.is_none() {
            return;
        }
        self.submenu_grace = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(motion::MENU_SUBMENU_GRACE)
                .await;
            let _ = this.update(cx, |this, cx| this.close_submenu(cx));
        }));
    }

    /// The width a submenu's rows need (`width: auto`).
    fn submenu_width(&self, top: usize, window: &Window) -> f32 {
        let rows = self.submenu_rows(top);
        let has_icons = self.has_icons(&rows);
        let measure = |text: &SharedString, size: f32| {
            let run = TextRun {
                len: text.len(),
                font: font(theme::FONT_UI),
                color: black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(text.clone(), px(size), &[run], None)
                    .width,
            )
        };
        let widest = rows
            .iter()
            .filter_map(|row| match row {
                Row::Entry(path) => match self.entry(*path) {
                    Entry::Item(item) => Some(
                        2. * ROW_PADDING
                            + if has_icons { ICON_SLOT } else { 0. }
                            + measure(&item.label, theme::TEXT_MD)
                            + if item.checked.is_some() || item.excluded {
                                CHECK_SLOT
                            } else {
                                0.
                            }
                            + item.shortcut.as_ref().map_or(0., |shortcut| {
                                SHORTCUT_GAP + measure(shortcut, theme::TEXT_CAPTION)
                            }),
                    ),
                    _ => None,
                },
                Row::View(path, _) => match self.entry(*path) {
                    Entry::View(view) => Some(view.width - 2. * PADDING),
                    _ => None,
                },
                Row::Separator | Row::Swatches(_) => None,
            })
            .fold(0f32, f32::max);
        (2. * PADDING + widest).ceil()
    }

    fn open_submenu(
        &mut self,
        row: usize,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing.is_some() {
            return;
        }
        self.submenu_grace = None;
        if let Some((open_row, top, inside)) = self.opened()
            && open_row == row
        {
            if keyboard && inside.is_none() {
                let first = step(&self.stops(&self.submenu_rows(top)), None, true);
                self.set_submenu_current(first, cx);
            }
            return;
        }
        let rows = self.rows();
        let Some(Row::Entry(path)) = rows.get(row).copied() else {
            return;
        };
        if !matches!(self.entry(path), Entry::Submenu(_)) {
            return;
        }
        self.close_submenu(cx);
        let (area, _) = self.area();
        let parent = self.root_rect(&rows);
        let row_top = parent.y
            + self.search_height()
            + row_offset(&rows, row)
            + f32::from(self.scroll.offset().y);
        let children = self.submenu_rows(path.top);
        let width = self.submenu_width(path.top, window);
        let height = items_height(&children, false).min(area.h - VIEWPORT_INSET);
        let (x, y) = place_submenu(parent, row_top, width, height, area);
        let rect = Rect {
            x,
            y,
            w: width,
            h: height,
        };
        self.serial += 1;
        self.submenu_highlights = motion::Fades::default();
        self.submenu = Some(Opened {
            row,
            top: path.top,
            current: None,
            rect,
            window: None,
            bounds: rect.outset(SHADOW_ROOM).intersect(area),
            serial: self.serial,
        });
        if keyboard {
            let first = step(&self.stops(&children), None, true);
            self.set_submenu_current(first, cx);
        }
        if self.presentation == Presentation::PopUpWindows {
            self.open_pop_up(Role::Submenu(self.serial), cx);
        }
        cx.notify();
    }

    fn close_submenu(&mut self, cx: &mut Context<Self>) {
        self.submenu_grace = None;
        if let Some(open) = self.submenu.take() {
            if let Some(window) = open.window {
                close_window(window, cx);
            }
            cx.notify();
        }
    }

    // ── Pop-up windows ──────────────────────────────────────────────

    /// Finds the display and the owner window's place on it, and opens the
    /// root panel's window. Falls back to the overlay when there is no
    /// display to place it on.
    fn open_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(display) = window.display(cx) else {
            self.presentation = Presentation::InWindow;
            return;
        };
        // `Window::bounds` is the frame on its display; the content starts
        // below any system titlebar, at the frame's bottom less its height.
        let frame = window.bounds();
        let content = window.viewport_size();
        let origin = (
            f32::from(frame.origin.x),
            f32::from(frame.origin.y) + f32::from(frame.size.height - content.height),
        );
        let area = Rect::from_bounds(display.visible_bounds());
        self.screen = Some(Screen {
            area,
            origin,
            display: display.id(),
            bounds: Rect::default(),
            window: None,
        });
        let (area, pointer) = self.area();
        let tallest = self.tallest();
        let (x, y) = place(pointer, self.width, tallest, area);
        let rect = Rect {
            x,
            y,
            w: self.width,
            h: tallest.min(area.h - VIEWPORT_INSET),
        };
        if let Some(screen) = &mut self.screen {
            screen.bounds = rect.outset(SHADOW_ROOM).intersect(area);
        }
        self.open_pop_up(Role::Root, cx);
    }

    /// Opens a pop-up window for the root panel or the open submenu, once
    /// this update is over: the window draws the menu as it opens.
    fn open_pop_up(&mut self, role: Role, cx: &mut Context<Self>) {
        let Some(screen) = &self.screen else {
            return;
        };
        let bounds = match role {
            Role::Root => screen.bounds,
            Role::Submenu(_) => match &self.submenu {
                Some(open) => open.bounds,
                None => return,
            },
        };
        let display = screen.display;
        let focus = match role {
            Role::Root => self.focus_handle(cx),
            Role::Submenu(_) => self.focus.clone(),
        };
        let menu = cx.entity();
        cx.defer(move |cx| {
            let still_wanted = menu.update(cx, |this, _| {
                let wanted = this.closing.is_none()
                    && match role {
                        Role::Root => true,
                        Role::Submenu(serial) => this
                            .submenu
                            .as_ref()
                            .is_some_and(|open| open.serial == serial),
                    };
                if wanted {
                    this.opening = Some(role);
                }
                wanted
            });
            if !still_wanted {
                return;
            }
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds.to_bounds())),
                display_id: Some(display),
                titlebar: None,
                focus: false,
                show: true,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            };
            let view = menu.clone();
            let opened = cx
                .open_window(options, move |window, cx| {
                    without_system_shadow(window);
                    window.focus(&focus, cx);
                    view
                })
                .ok()
                .map(AnyWindowHandle::from);
            menu.update(cx, |this, cx| {
                this.opening = None;
                match (role, opened) {
                    (_, Some(handle)) if this.closing.is_some() => close_window(handle, cx),
                    (Role::Root, Some(handle)) => {
                        register(handle, cx);
                        if let Some(screen) = &mut this.screen {
                            screen.window = Some(handle);
                        }
                    }
                    (Role::Submenu(serial), Some(handle)) => match &mut this.submenu {
                        Some(open) if open.serial == serial => {
                            register(handle, cx);
                            open.window = Some(handle);
                        }
                        _ => close_window(handle, cx),
                    },
                    // The platform has no pop-up windows: draw in the window.
                    (Role::Root, None) => {
                        this.presentation = Presentation::InWindow;
                        this.screen = None;
                    }
                    (Role::Submenu(_), None) => this.close_submenu(cx),
                }
                cx.notify();
            });
        });
    }

    /// Closes the pop-up windows. If one of them had taken the keyboard (a
    /// click in a panel makes it the key window), it goes back to the owner.
    fn close_pop_ups(&mut self, cx: &mut Context<Self>) {
        let mut windows = Vec::new();
        if let Some(screen) = &mut self.screen {
            windows.extend(screen.window.take());
        }
        if let Some(open) = &mut self.submenu {
            windows.extend(open.window.take());
        }
        let main = self.main;
        cx.defer(move |cx| {
            let mut watched = windows.clone();
            watched.extend(children_of(&windows, cx));
            let had_keyboard = watched.iter().any(|window| {
                window
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false)
            });
            for window in windows {
                forget(window, cx);
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            if had_keyboard && let Some(main) = main {
                let _ = main.update(cx, |_, window, _| window.activate_window());
            }
        });
    }

    /// Closes the menu when the keyboard leaves all of its windows: another
    /// application was chosen, or the owner window was. A pop-up stays
    /// above other applications' windows, so it must not outlive that.
    fn watch_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = window.window_handle();
        if self.presentation != Presentation::PopUpWindows || self.watched.contains(&handle) {
            return;
        }
        self.watched.push(handle);
        let subscription = cx.observe_window_activation(window, |this, _, cx| {
            let mut windows = this.main.into_iter().collect::<Vec<_>>();
            windows.extend(this.screen.as_ref().and_then(|screen| screen.window));
            windows.extend(this.submenu.as_ref().and_then(|open| open.window));
            // A select's list opened from a panel counts as the menu's.
            windows.extend(children_of(&windows, cx));
            // When a click moves the keyboard from one of the menu's windows
            // to another, the one losing it reports first; wait for the
            // other before deciding.
            this.activation = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ACTIVATION_SETTLE).await;
                let active = windows.iter().any(|window| {
                    window
                        .update(cx, |_, window, _| window.is_window_active())
                        .unwrap_or(false)
                });
                if !active {
                    let _ = this.update(cx, |this, cx| this.dismiss(cx));
                }
            }));
        });
        self._subscriptions.push(subscription);
    }

    // ── Drawing ─────────────────────────────────────────────────────

    /// The menu's keys, on whichever of its windows has the keyboard. The
    /// search field binds Enter, Escape and the side arrows itself; they are
    /// taken here before it sees them.
    fn keyboard(&self, element: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        element
            .track_focus(&self.focus)
            .key_context("Menu")
            .on_action(cx.listener(|this, _: &Up, _, cx| this.arrow(false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.arrow(true, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.dismiss(cx)))
            .on_action(cx.listener(|this, _: &OpenSubmenu, window, cx| {
                this.right(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseSubmenu, _, cx| {
                this.left(cx);
            }))
            .capture_action(cx.listener(|this, _: &text_input::Commit, window, cx| {
                // A field in an owner's row keeps its Enter unless a row is
                // highlighted.
                let in_row = this.current.is_some()
                    || this.opened().is_some_and(|(_, _, inside)| inside.is_some());
                let in_search = this
                    .search
                    .as_ref()
                    .is_some_and(|search| search.focus_handle(cx).is_focused(window));
                if in_row || in_search {
                    this.confirm(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &text_input::Cancel, _, cx| {
                this.dismiss(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &text_input::Right, window, cx| {
                if this.right(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &text_input::Left, _, cx| {
                if this.left(cx) {
                    cx.stop_propagation();
                }
            }))
    }

    /// While open, a press outside the panels closes the menu, with any
    /// button, and goes no further.
    fn press_outside(&self, element: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        if self.closing.is_some() {
            return element;
        }
        element
            .occlude()
            .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.dismiss(cx);
                cx.stop_propagation();
            }))
    }

    /// The glass panel of `w` × `h` at `at` in its window, holding `content`,
    /// drawn at `scale` about `origin` and at `opacity`. GPUI cannot scale
    /// the contents, so the panel's surface, border and shadow are drawn in
    /// the scaled rectangle and the contents stay at full size and in place,
    /// clipped to it.
    #[allow(clippy::too_many_arguments)]
    fn chrome(
        &self,
        id: &'static str,
        at: (f32, f32),
        w: f32,
        h: f32,
        (scale, opacity): (f32, f32),
        origin: (f32, f32),
        content: Div,
        cx: &App,
    ) -> Stateful<Div> {
        let (dx, dy, scaled_w, scaled_h) = motion::scaled_frame(w, h, scale, origin);
        // `.floatingGlassSurface`: the glass, with its edges.
        super::glass_panel(id, theme::RADIUS_LG, cx)
            .occlude()
            .absolute()
            .left(px(at.0 + dx))
            .top(px(at.1 + dy))
            .w(px(scaled_w))
            .h(px(scaled_h))
            .overflow_hidden()
            .opacity(opacity)
            // A press on the panel itself is not a press outside it.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(
                content
                    .absolute()
                    .left(px(-dx))
                    .top(px(-dy))
                    .w(px(w))
                    .h(px(h))
                    .flex()
                    .flex_col(),
            )
            .into_div()
    }

    /// A row as `.item` draws it, without what it does.
    fn row(
        &self,
        id: impl Into<ElementId>,
        entry: &Entry,
        highlight: f32,
        current: bool,
        has_icons: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let (label, icon) = match entry {
            Entry::Item(item) => (item.label.clone(), item.icon),
            Entry::Submenu(submenu) => (submenu.label.clone(), submenu.icon),
            Entry::Separator | Entry::Swatches(_) | Entry::View(_) => {
                (SharedString::default(), None)
            }
        };
        let item = match entry {
            Entry::Item(item) => Some(item),
            _ => None,
        };
        let danger = item.is_some_and(|item| item.danger);
        let excluded = item.is_some_and(|item| item.excluded);
        let color = if danger || excluded {
            theme.negative()
        } else {
            theme.text()
        };
        let mut row = div()
            .id(id)
            .relative()
            .h(px(theme::CONTROL_SM))
            .flex_none()
            .px(px(ROW_PADDING))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .text_color(color)
            .whitespace_nowrap()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .bg(if danger {
                        theme.negative_at(12.)
                    } else {
                        theme.hover_overlay()
                    })
                    .opacity(highlight),
            );
        if has_icons {
            row = row.child(
                div()
                    .relative()
                    .w(px(16.))
                    .mr(px(6.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .children(icon.map(|icon| {
                        super::icon(icon, 16., color).opacity(if current { 1. } else { 0.8 })
                    })),
            );
        }
        let mut text = div().relative().flex_1().min_w_0().truncate().child(label);
        if excluded {
            text = text.line_through();
        }
        row = row.child(text);
        if let Some(item) = item {
            // `.checkSlot`, in the primary text colour on any row. Its glyph is
            // drawn at 16px: `.item svg` sizes every glyph in a row.
            if item.checked.is_some() || item.excluded {
                let glyph = if item.excluded {
                    Some("menu-x")
                } else if item.checked == Some(true) {
                    Some("menu-check")
                } else {
                    None
                };
                row = row.child(
                    div()
                        .relative()
                        .size(px(CHECK_SLOT))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(glyph.map(|glyph| super::icon(glyph, 16., theme.text()))),
                );
            }
            if let Some(shortcut) = &item.shortcut {
                row = row.child(
                    div()
                        .relative()
                        .flex_none()
                        .pl(px(SHORTCUT_GAP))
                        .text(theme::TEXT_CAPTION)
                        .opacity(0.5)
                        .child(shortcut.clone()),
                );
            }
            if item.disabled {
                return row.opacity(0.4);
            }
        } else {
            // `.chevron`: 40% of the row's colour, and 16px like every
            // glyph in a row.
            row = row.child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(super::icon("menu-chevron-right", 16., color).opacity(0.4)),
            );
        }
        row.cursor_pointer()
    }

    /// The menu's own panel: the search row, then its rows, scrolling when
    /// the panel is held shorter than they are.
    fn root_panel(
        &self,
        rows: &[Row],
        rect: Rect,
        at: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let has_icons = self.has_icons(rows);
        let mut list = div().flex().flex_col().p(px(PADDING)).gap(px(GAP));
        for (index, row) in rows.iter().enumerate() {
            let path = match *row {
                Row::Entry(path) => path,
                Row::Swatches(path) => {
                    list = list.child(self.swatch_row(path, window, cx));
                    continue;
                }
                Row::View(path, height) => {
                    list = list.child(self.view_row(path, height, cx));
                    continue;
                }
                Row::Separator => {
                    list = list.child(separator(cx));
                    continue;
                }
            };
            let entry = self.entry(path);
            let element = self.row(
                ("menu-row", index),
                entry,
                self.highlights.value(("row", index)),
                self.current == Some(index),
                has_icons,
                cx,
            );
            list = list.child(match entry {
                Entry::Item(item) if item.disabled => element,
                Entry::Item(_) => element
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_current(Some(index), cx);
                            this.leave_submenu(cx);
                        } else if this.current == Some(index) {
                            this.set_current(None, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| this.choose_context(path, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(path, cx))),
                // A submenu row keeps its highlight when the pointer leaves
                // it for the submenu.
                _ => element
                    .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                        if *hovered {
                            this.set_current(Some(index), cx);
                            this.open_submenu(index, false, window, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_submenu(index, false, window, cx)
                    })),
            });
        }
        if rows.is_empty() && self.searching() {
            list = list.child(
                div()
                    .py(px(8.))
                    .px(px(12.))
                    .flex()
                    .justify_center()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child("No results"),
            );
        }
        let mut panel = div();
        if let Some(search) = &self.search {
            let mut glyph = theme.text_tertiary();
            glyph.a *= 0.6;
            panel = panel.child(
                div()
                    .relative()
                    .h(px(SEARCH_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(px(10.))
                    .gap(px(6.))
                    // `box-shadow: 0 -1px 0 var(--color-border-secondary) inset`
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(-1.), theme.border_secondary().into()).inset(),
                    ])
                    .child(super::icon("menu-search", 16., glyph).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(SEARCH_HEIGHT))
                            .text_color(theme.text())
                            .child(search.clone()),
                    ),
            );
        }
        // A menu taller than its area scrolls, and then shows the app's
        // scrollbar, whose 8px gutter narrows the rows (`::-webkit-scrollbar`).
        let overflows = self.height(rows) > rect.h + 0.5;
        let items_height = (rect.h - self.search_height()).max(0.);
        panel = panel.child(
            div()
                .relative()
                .h(px(items_height))
                .child(
                    div()
                        .id("menu-items")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .when(overflows, |items| items.pr(px(super::scrollbar::GUTTER)))
                        .child(list),
                )
                .when(overflows, |items| {
                    items.child(super::scrollbar("menu-scrollbar", &self.scroll, window, cx))
                }),
        );
        self.chrome(
            "menu",
            at,
            rect.w,
            rect.h,
            self.root_motion(),
            self.root_origin(rows),
            panel,
            cx,
        )
    }

    /// The open submenu's panel.
    fn submenu_panel(
        &self,
        open: &Opened,
        at: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let rows = self.submenu_rows(open.top);
        let has_icons = self.has_icons(&rows);
        let mut list = div().flex().flex_col().p(px(PADDING)).gap(px(GAP));
        for (index, row) in rows.iter().enumerate() {
            let path = match *row {
                Row::Entry(path) => path,
                Row::Swatches(path) => {
                    list = list.child(self.swatch_row(path, window, cx));
                    continue;
                }
                Row::View(path, height) => {
                    list = list.child(self.view_row(path, height, cx));
                    continue;
                }
                Row::Separator => {
                    list = list.child(separator(cx));
                    continue;
                }
            };
            let entry = self.entry(path);
            let element = self.row(
                ("submenu-row", index),
                entry,
                self.submenu_highlights.value(("row", index)),
                open.current == Some(index),
                has_icons,
                cx,
            );
            list = list.child(match entry {
                Entry::Item(item) if !item.disabled => element
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let current = this.submenu.as_ref().and_then(|open| open.current);
                        if *hovered {
                            this.set_submenu_current(Some(index), cx);
                        } else if current == Some(index) {
                            this.set_submenu_current(None, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| this.choose_context(path, cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(path, cx))),
                _ => element,
            });
        }
        let content = div().child(
            div()
                .id("submenu-items")
                .relative()
                .h(px(open.rect.h))
                .overflow_y_scroll()
                .child(list),
        );
        self.chrome(
            "submenu",
            at,
            open.rect.w,
            open.rect.h,
            (1., 1.),
            (0., 0.),
            content,
            cx,
        )
        // Reaching the submenu keeps it open.
        .on_hover(cx.listener(|this, hovered: &bool, _, _| {
            if *hovered {
                this.submenu_grace = None;
            }
        }))
    }

    /// The owner window's part: the layer that takes the keyboard and
    /// presses outside, and, drawn in the window, the panels.
    fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = window.viewport_size();
        self.viewport = Rect {
            x: 0.,
            y: 0.,
            w: f32::from(view.width),
            h: f32::from(view.height),
        };
        if self.presentation == Presentation::PopUpWindows
            && self.screen.is_none()
            && self.closing.is_none()
        {
            self.open_screen(window, cx);
        }
        self.watch_activation(window, cx);
        let layer = self.keyboard(div().id("menu-layer").absolute().inset_0(), cx);
        let mut layer = self.press_outside(layer, cx);
        match self.presentation {
            Presentation::InWindow => {
                self.shown.get_or_insert_with(Instant::now);
                let rows = self.rows();
                let rect = self.root_rect(&rows);
                layer = layer.child(self.root_panel(&rows, rect, (rect.x, rect.y), window, cx));
                if let Some(open) = &self.submenu {
                    layer = layer.child(self.submenu_panel(
                        open,
                        (open.rect.x, open.rect.y),
                        window,
                        cx,
                    ));
                }
            }
            Presentation::PopUpWindows => {
                // The keyboard stays with this window, so the search field
                // types here: a twin of it, invisible, at the same place in
                // this window as the one shown in the pop-up, so both lay the
                // text out alike.
                if let (Some(search), Some(screen)) = (&self.search, &self.screen) {
                    let rect = self.root_rect(&self.rows());
                    layer = layer.child(
                        div()
                            .absolute()
                            .left(px(rect.x - screen.bounds.x + SEARCH_INPUT_LEFT))
                            .top(px(rect.y - screen.bounds.y))
                            .w(px(rect.w - SEARCH_INPUT_LEFT))
                            .h(px(SEARCH_HEIGHT))
                            .opacity(0.)
                            .child(search.clone()),
                    );
                }
            }
        }
        layer.into_any_element()
    }

    /// A pop-up window's part: a transparent window holding one panel.
    fn render_pop_up(
        &mut self,
        role: Role,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.watch_activation(window, cx);
        let theme = theme::theme(cx);
        let root = div()
            .id("menu-window")
            .size_full()
            .font_family(theme::FONT_UI)
            .text(theme::TEXT_MD)
            .text_color(theme.text());
        let root = self.keyboard(root, cx);
        let mut root = self.press_outside(root, cx);
        match role {
            Role::Root => {
                if self.screen.is_some() {
                    self.shown.get_or_insert_with(Instant::now);
                }
                if let Some(screen) = &self.screen {
                    let rows = self.rows();
                    let rect = self.root_rect(&rows);
                    let at = (rect.x - screen.bounds.x, rect.y - screen.bounds.y);
                    root = root.child(self.root_panel(&rows, rect, at, window, cx));
                }
            }
            Role::Submenu(serial) => {
                if let Some(open) = self.submenu.as_ref().filter(|open| open.serial == serial) {
                    let at = (open.rect.x - open.bounds.x, open.rect.y - open.bounds.y);
                    root = root.child(self.submenu_panel(open, at, window, cx));
                }
            }
        }
        root.into_any_element()
    }
}

impl Menu {
    /// Picks a swatch: the owner is told, and the menu stays open with the
    /// new choice ringed.
    fn pick_swatch(&mut self, path: Path, swatch: usize, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        let entry = match (&mut self.entries[path.top], path.child) {
            (Entry::Submenu(submenu), Some(child)) => &mut submenu.children[child],
            (entry, _) => entry,
        };
        if let Entry::Swatches(row) = entry
            && let Some(choice) = row.swatches.get(swatch)
        {
            row.selected = Some(swatch);
            self.chosen = Some(choice.value.clone());
            cx.emit(Event::Chosen(row.action));
            cx.notify();
        }
    }

    /// `ColorPicker`: swatches spread across the row, the chosen one ringed
    /// in its colour over the app background, with a check.
    fn swatch_row(&self, path: Path, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let Entry::Swatches(row) = self.entry(path) else {
            return div().id("menu-swatches");
        };
        let mut line = div()
            .id(("menu-swatches", path.top))
            .relative()
            .h(px(SWATCH_ROW))
            .flex_none()
            .px(px(ROW_PADDING))
            .flex()
            .items_center()
            .justify_between()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.set_current(None, cx);
                    this.leave_submenu(cx);
                }
            }));
        for (position, swatch) in row.swatches.iter().enumerate() {
            let color = swatch.color.unwrap_or(theme.text());
            let selected = row.selected == Some(position);
            let check = if swatch.color.is_some() {
                theme.on_primary()
            } else {
                theme.background()
            };
            let mut ring = div()
                .size(px(SWATCH))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(color)
                .children(selected.then(|| super::icon("swatch-check", 10., check)));
            if selected {
                // `box-shadow: 0 0 0 1.5px var(--color-bg-app), 0 0 0 3px <colour>`
                ring = ring.shadow(vec![
                    BoxShadow::new(px(0.), px(0.), theme.background().into())
                        .spread_radius(px(1.5)),
                    BoxShadow::new(px(0.), px(0.), color.into()).spread_radius(px(3.)),
                ]);
            }
            let target = div()
                .id(("menu-swatch", position))
                .cursor_pointer()
                .child(ring)
                .on_click(cx.listener(move |this, _, _, cx| this.pick_swatch(path, position, cx)));
            line = line.child(super::tooltip::attach(
                target,
                ("menu-swatch-tip", position),
                super::tip(swatch.label.clone()),
                SWATCH,
                window,
                cx,
            ));
        }
        line
    }
}

impl Menu {
    /// A view entry, at its own size. Reaching it takes the highlight off
    /// the menu's rows, as `.customItem`'s `onMouseEnter` does.
    fn view_row(&self, path: Path, height: u32, cx: &mut Context<Self>) -> Stateful<Div> {
        let Entry::View(entry) = self.entry(path) else {
            return div().id("menu-view");
        };
        div()
            .id((
                "menu-view",
                path.top * 1000 + path.child.map_or(0, |child| child + 1),
            ))
            .relative()
            .h(px(height as f32))
            .flex_none()
            .when(entry.padded, |row| row.px(px(CUSTOM_PADDING)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.set_current(None, cx);
                    this.leave_submenu(cx);
                }
            }))
            .child(entry.view.clone())
    }
}

fn separator(cx: &App) -> Div {
    div()
        .relative()
        .h(px(1.))
        .my(px(2.))
        .flex_none()
        .bg(theme::theme(cx).border_secondary())
}

/// Closes a window once the current update is over.
fn close_window(window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        forget(window, cx);
        let _ = window.update(cx, |_, window, _| window.remove_window());
    });
}

impl Render for Menu {
    /// The same menu draws in the owner's window and in each of its pop-up
    /// windows; which part it draws depends on the window.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.closing.is_some()
            || self.entering()
            || self.highlights.animating()
            || self.submenu_highlights.animating()
        {
            window.request_animation_frame();
        }
        let handle = window.window_handle();
        if self.main.is_none() {
            self.main = Some(handle);
        }
        if self.main == Some(handle) {
            return self.render_main(window, cx);
        }
        let role = if self.screen.as_ref().and_then(|screen| screen.window) == Some(handle) {
            Some(Role::Root)
        } else if let Some(open) = self
            .submenu
            .as_ref()
            .filter(|open| open.window == Some(handle))
        {
            Some(Role::Submenu(open.serial))
        } else {
            // A window drawn as it opens, before its handle is known.
            self.opening
        };
        match role {
            Some(role) => self.render_pop_up(role, window, cx),
            // A submenu window already replaced, about to close.
            None => div().into_any_element(),
        }
    }
}

/// A transparent window still gets the system's own shadow and edge
/// highlight, traced round whatever it draws, which shows as a second, glassy
/// border round the panel. The panel draws its own shadow, so the system's
/// is turned off. GPUI has no option for this, so it asks AppKit directly.
#[cfg(target_os = "macos")]
pub(crate) fn without_system_shadow(window: &Window) {
    use super::window_chrome::appkit;
    appkit::with_ns_window(window, |ns_window| {
        // SAFETY: `setHasShadow:` is an NSWindow method taking a BOOL,
        // called on the main thread.
        unsafe {
            let send_bool: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void, bool) =
                std::mem::transmute(appkit::send());
            send_bool(ns_window, appkit::selector(c"setHasShadow:"), false);
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn without_system_shadow(_: &Window) {}

#[cfg(test)]
mod tests {
    use super::{
        Entry, MARGIN, PADDING, Path, Rect, Row, SEPARATOR, clean, item, items_height, origin,
        place, place_submenu, row_offset, score, search, step, submenu, swatches,
    };

    fn top(index: usize) -> Path {
        Path {
            top: index,
            child: None,
        }
    }

    fn child(top: usize, child: usize) -> Path {
        Path {
            top,
            child: Some(child),
        }
    }

    fn grid_like() -> Vec<Entry> {
        vec![
            item("open", "Open with Default App").into(),
            item("reveal", "Reveal in Finder")
                .keywords(["show", "folder"])
                .into(),
            Entry::Separator,
            item("copy-path", "Copy File Path").into(),
            item("copy-name", "Copy Name").into(),
            Entry::Separator,
            submenu(
                "Set Rating",
                vec![
                    item("rate-0", "No Rating").into(),
                    item("rate-1", "★").into(),
                ],
            )
            .icon("menu-star")
            .into(),
            item("delete", "Delete Permanently").disabled(true).into(),
            item("trash", "Move to Trash").into(),
        ]
    }

    #[test]
    fn search_ranks_label_starts_then_contains_then_keywords_then_letters() {
        let entries = grid_like();
        // "co": both copies start with it; earlier in the menu first.
        assert_eq!(search(&entries, "co"), vec![top(3), top(4)]);
        // "name": inside a label.
        assert_eq!(search(&entries, " Name "), vec![top(4)]);
        // "folder": a keyword only.
        assert_eq!(search(&entries, "folder"), vec![top(1)]);
        // "mvt": the letters of "Move to Trash" in order, with gaps.
        assert_eq!(search(&entries, "mvt"), vec![top(8)]);
        assert_eq!(score("move to trash", "move to trash", "move"), Some(0));
        assert_eq!(score("copy name", "copy name", "name"), Some(15));
        assert_eq!(
            score("reveal in finder show", "reveal in finder", "show"),
            Some(117)
        );
        assert_eq!(score("abc", "abc", "xyz"), None);
    }

    #[test]
    fn search_skips_disabled_items_and_finds_submenus_and_their_rows() {
        let entries = grid_like();
        // The disabled delete row never matches.
        assert_eq!(search(&entries, "delete"), Vec::<Path>::new());
        // A submenu's own row and its children are both results.
        assert_eq!(search(&entries, "rating"), vec![child(6, 0), top(6)]);
        assert_eq!(search(&entries, "set"), vec![top(6)]);
        assert_eq!(search(&entries, "★"), vec![child(6, 1)]);
        assert_eq!(search(&entries, "   "), Vec::<Path>::new());
    }

    #[test]
    fn separators_never_lead_trail_or_double() {
        let entries: Vec<Entry> = vec![
            Entry::Separator,
            item("a", "A").into(),
            Entry::Separator,
            Entry::Separator,
            item("b", "B").into(),
            Entry::Separator,
        ];
        assert_eq!(
            clean(&entries, None),
            vec![Row::Entry(top(1)), Row::Separator, Row::Entry(top(4))]
        );
        // A row that is not an item (a panel or swatches) keeps the
        // separator after it, as the view menu's panels have one between.
        let entries: Vec<Entry> = vec![
            swatches("colour", Vec::new(), None).into(),
            Entry::Separator,
            item("b", "B").into(),
        ];
        assert_eq!(
            clean(&entries, None),
            vec![Row::Swatches(top(0)), Row::Separator, Row::Entry(top(2))]
        );
    }

    #[test]
    fn arrows_step_over_separators_and_disabled_rows_and_stop_on_submenus() {
        // open, reveal, ─, copy path, copy name, ─, Set Rating ›, delete (disabled), trash
        let stops = [true, true, false, true, true, false, true, false, true];
        assert_eq!(step(&stops, None, true), Some(0));
        assert_eq!(step(&stops, None, false), Some(8));
        assert_eq!(step(&stops, Some(1), true), Some(3));
        assert_eq!(step(&stops, Some(4), true), Some(6));
        assert_eq!(step(&stops, Some(6), true), Some(8));
        assert_eq!(step(&stops, Some(8), true), Some(0));
        assert_eq!(step(&stops, Some(0), false), Some(8));
        assert_eq!(step(&stops, Some(8), false), Some(6));
        // Nothing to stop on leaves the highlight where it was.
        assert_eq!(step(&[false, false], Some(1), true), Some(1));
    }

    #[test]
    fn heights_count_rows_separators_gaps_and_the_empty_line() {
        let rows = [Row::Entry(top(0)), Row::Separator, Row::Entry(top(2))];
        assert_eq!(
            items_height(&rows, false),
            2. * PADDING + 26. + SEPARATOR + 26. + 2. * 2.
        );
        assert_eq!(items_height(&[], true), 2. * PADDING + 30.);
        assert_eq!(row_offset(&rows, 2), PADDING + 26. + 2. + SEPARATOR + 2.);
    }

    #[test]
    fn a_menu_grows_from_the_corner_it_was_placed_by() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        // Room at the pointer: from the top-left corner, at the pointer.
        assert_eq!(origin((100., 100.), 270., 300., area), (0., 0.));
        // Moved in from the right edge: from the right; from the bottom too.
        assert_eq!(origin((900., 100.), 270., 300., area), (1., 0.));
        assert_eq!(origin((900., 700.), 270., 300., area), (1., 1.));
        // Exactly at the margin still fits.
        assert_eq!(
            origin(
                (1000. - 270. - MARGIN, 800. - 300. - MARGIN),
                270.,
                300.,
                area
            ),
            (0., 0.)
        );
        // The scaled frame then keeps that corner where `place` put it.
        let (x, y) = place((900., 700.), 270., 300., area);
        let (dx, dy, w, h) = crate::motion::scaled_frame(270., 300., 0.96, (1., 1.));
        assert!((x + dx + w - (x + 270.)).abs() < 1e-3);
        assert!((y + dy + h - (y + 300.)).abs() < 1e-3);
    }

    #[test]
    fn a_menu_moves_inward_to_keep_its_margin() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        // Room at the pointer.
        assert_eq!(place((100., 100.), 270., 300., area), (100., 100.));
        // Near the right and bottom edges it moves left and up.
        assert_eq!(
            place((900., 700.), 270., 300., area),
            (1000. - 270. - MARGIN, 800. - 300. - MARGIN)
        );
        // Taller than the area: held at the top margin.
        assert_eq!(place((50., 50.), 270., 900., area).1, MARGIN);
        // An area that does not start at zero, like a display's usable part
        // below the menu bar.
        let display = Rect {
            x: 0.,
            y: 25.,
            w: 1000.,
            h: 775.,
        };
        assert_eq!(place((5., 0.), 270., 100., display), (MARGIN, 25. + MARGIN));
    }

    #[test]
    fn a_submenu_opens_right_of_its_row_and_flips_left_at_the_edge() {
        let area = Rect {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        let parent = Rect {
            x: 100.,
            y: 100.,
            w: 270.,
            h: 400.,
        };
        assert_eq!(
            place_submenu(parent, 160., 150., 200., area),
            (100. + 270. + 4., 157.)
        );
        // Too close to the right edge: on the parent's left.
        let right = Rect { x: 700., ..parent };
        assert_eq!(
            place_submenu(right, 160., 150., 200., area),
            (700. - 150. - 4., 157.)
        );
        // Low rows move up to keep the margin; high ones down.
        assert_eq!(
            place_submenu(parent, 700., 150., 200., area).1,
            800. - 200. - MARGIN
        );
        assert_eq!(place_submenu(parent, 5., 150., 200., area).1, MARGIN);
    }

    #[test]
    fn rects_grow_for_shadows_and_clip_to_the_display() {
        let panel = Rect {
            x: 10.,
            y: 20.,
            w: 100.,
            h: 50.,
        };
        let area = Rect {
            x: 0.,
            y: 25.,
            w: 1000.,
            h: 800.,
        };
        assert_eq!(
            panel.outset(40.).intersect(area),
            Rect {
                x: 0.,
                y: 25.,
                w: 150.,
                h: 85.,
            }
        );
    }
}
