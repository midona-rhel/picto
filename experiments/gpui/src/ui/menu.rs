//! The context menu (`ContextMenu.tsx`): a glass panel of rows at the
//! pointer, with check marks, submenus and a search row.
//!
//! A menu is shown one of two ways, and `presentation()` alone chooses:
//! drawn over the owner's window and kept 12px inside it, or with each panel
//! (the menu and its open submenu) in a pop-up window of its own, kept 12px
//! inside the display, so a menu can reach past the window's edge. In both,
//! the keyboard stays with the owner's window, where the menu's layer takes
//! it, and a press anywhere outside the panels closes the menu.
use super::{color_picker, glass_panel, icon, scrollbar, tip, tooltip};
use crate::ui::text_input::TextInput;
use crate::{motion, theme};
use gpui::*;
use std::time::{Duration, Instant};

mod entries;
pub(crate) mod host;
mod layout;
mod popup;
mod view;

pub use entries::*;
pub use host::{Host, Return};
use layout::{
    Path, Rect, Row, clean, items_height, origin, place, place_submenu, row_offset, search, step,
};
use popup::close_window;
pub(crate) use popup::without_system_shadow;
pub use popup::{adopt, is_pop_up, release};

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

    /// Replaces the top-level row labelled `label` with `replacement`
    /// (none, to take it away) while the menu is open (`replaceEntry`), for
    /// a row whose contents arrive after the menu opened. A submenu open at
    /// or below it closes, since its rows have moved.
    pub fn replace_entry(&mut self, label: &str, replacement: Vec<Entry>, cx: &mut Context<Self>) {
        let found = self.entries.iter().position(|entry| match entry {
            Entry::Item(item) => item.label.as_ref() == label,
            Entry::Submenu(submenu) => submenu.label.as_ref() == label,
            Entry::Separator | Entry::Swatches(_) | Entry::View(_) => false,
        });
        let Some(index) = found else {
            return;
        };
        if self.submenu.as_ref().is_some_and(|open| open.top >= index) {
            self.close_submenu(cx);
        }
        self.entries.splice(index..=index, replacement);
        self.current = None;
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

    /// The root panel's opacity now: fading in once it is first drawn where
    /// the system does not scale it in (see `pop_in`), fading out once
    /// dismissed. GPUI cannot scale an element's contents, and a panel that
    /// grows around contents standing still does not read as `menuIn`'s
    /// scale, so the panel itself never changes size. Submenus do not
    /// animate (`.submenuMenu { animation: none }`): an open one stays as it
    /// is until the menu is gone.
    fn root_motion(&self) -> (f32, f32) {
        let opacity = match (self.closing, self.shown) {
            (Some(since), _) => motion::menu_out(since.elapsed()).1,
            // Its window's layer is scaled in instead.
            (None, _) if self.presentation == Presentation::PopUpWindows => 1.,
            (None, Some(shown)) => {
                motion::EASE_SCALE_IN.ease(motion::progress(shown.elapsed(), motion::MENU_IN))
            }
            (None, None) => 0.,
        };
        (1., opacity)
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
                Entry::Item(item) => item.icon.is_some() || item.image.is_some(),
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
            bounds: Rect::submenu_window(rect, parent, SHADOW_ROOM, area),
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
}
