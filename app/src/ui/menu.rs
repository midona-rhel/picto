//! Draws the context menu: a glass panel of rows that opens at the pointer.
//! A row can have a check mark or open a submenu, and a menu of seven or
//! more rows has a search row at its top.
//!
//! A menu is shown in one of two ways, and only `presentation()` chooses
//! between them. Either its panels are drawn over the owner's window and
//! kept 12px inside it, or each panel (the menu and its open submenu) is in
//! a pop-up window of its own and kept 12px inside the display, so that a
//! menu can reach past the window's edge. In both ways the keyboard stays
//! with the owner's window, where the menu's layer handles the keys, and a
//! press anywhere outside the panels closes the menu.
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

/// Binds the menu's keys. The up and down arrows move the highlight, the
/// right and left arrows open and close a submenu, Enter chooses the
/// highlighted row and Escape closes the menu.
pub fn bind(cx: &mut App) {
    use super::keys::{self, Key};
    keys::bind(
        cx,
        "Menu",
        &[
            Key::Up,
            Key::Down,
            Key::Enter,
            Key::Escape,
            Key::Right,
            Key::Left,
        ],
    );
}

/// Says how a menu is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// The panels are drawn over the owner's window and kept inside it.
    InWindow,
    /// Each panel is in a pop-up window of its own and is kept inside the
    /// display.
    PopUpWindows,
}

/// Returns how menus are shown. No other code chooses between the two ways.
/// On macOS menus are in pop-up windows, because GPUI opens a
/// `WindowKind::PopUp` there as a non-activating panel, which can be shown
/// without taking the keyboard. On Windows and Linux, and on macOS when
/// `PICTO_GPUI_MENU=window` is set, menus are drawn inside the owner's
/// window. A menu whose pop-up window cannot be opened is also drawn inside
/// the owner's window.
///
/// Windows and Linux do not use pop-up windows for these reasons. A Wayland
/// client cannot place a top-level window (GPUI's `WindowKind::PopUp` is not
/// an `xdg_popup`). An X11 pop-up is an override-redirect window, which
/// takes no keyboard focus. On Windows, the keyboard and the activation of a
/// pop-up window have not been checked on a screen.
pub fn presentation() -> Presentation {
    let forced_in_window = crate::seams::menus_in_window();
    if cfg!(target_os = "macos") && !forced_in_window {
        Presentation::PopUpWindows
    } else {
        Presentation::InWindow
    }
}

const WIDTH: f32 = 270.;
/// The panel stays at least this far from the edges of the area it is kept
/// inside: the window, or the display for a menu in pop-up windows.
const MARGIN: f32 = 12.;
const PADDING: f32 = 4.;
const GAP: f32 = 2.;
/// The height of a separator: its 1px line with a 2px margin above and
/// below.
const SEPARATOR: f32 = 5.;
/// The height of the search row, and the distance from the panel's left
/// edge to the search field: 10px of padding, the 16px glyph and a 6px gap.
const SEARCH_HEIGHT: f32 = 36.;
const SEARCH_INPUT_LEFT: f32 = 32.;
/// The number of entries a menu needs before it has a search row.
/// Separators and entries that hold a view are not counted.
const SEARCH_THRESHOLD: usize = 7;
/// The height of the "No results" line that a search without results shows:
/// one line of 12px text with 8px above and below.
const EMPTY_HEIGHT: f32 = 30.;
/// A panel is at most as tall as the area it is kept inside, less this much.
const VIEWPORT_INSET: f32 = 40.;
/// A submenu opens `SUBMENU_GAP` to the right of its menu, with its top
/// `SUBMENU_LIFT` above the top of the row that opened it.
const SUBMENU_GAP: f32 = 4.;
const SUBMENU_LIFT: f32 = 3.;
/// The widths of the parts of a row: the padding at its left and right ends,
/// the icon with the gap after it, the check mark, and the space before a
/// shortcut.
const ROW_PADDING: f32 = 6.;
const ICON_SLOT: f32 = 22.;
const CHECK_SLOT: f32 = 16.;
const SHORTCUT_GAP: f32 = 28.;
/// The room a pop-up window leaves around its panel (a menu's panel, or a
/// select's list) for the panel's shadow. The shadow (`Theme::panel_shadow`)
/// reaches 24px out to the sides and 36px down.
pub(crate) const SHADOW_ROOM: f32 = 40.;
/// How long the menu waits, after one of its windows loses the keyboard, for
/// another of its windows to report that it now has the keyboard.
const ACTIVATION_SETTLE: Duration = Duration::from_millis(50);

/// The padding at the left and right of an entry that holds a view, when
/// the entry asks for padding (`ViewEntry::padded`).
const CUSTOM_PADDING: f32 = 6.;

/// The height of a row of colour swatches, and the size of one swatch. The
/// swatches are 18px with 4px above and below.
const SWATCH_ROW: f32 = 26.;
const SWATCH: f32 = 18.;

pub enum Event {
    /// A row was chosen. The value is the row's action. Unless the row keeps
    /// the menu open, the menu is already closing.
    Chosen(&'static str),
}

/// Holds the display that a menu in pop-up windows is shown on, and the
/// pop-up window of the menu's own panel.
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

/// Holds the state of the open submenu.
struct Opened {
    /// The index of the submenu's row among the menu's rows as drawn
    /// (`row`), and the index of its entry in the menu's entries (`top`).
    row: usize,
    top: usize,
    /// The row of the submenu that is under the pointer or was reached with
    /// the arrow keys. While this is set, the arrow keys move in the submenu.
    current: Option<usize>,
    /// The submenu's panel, in the menu's coordinates. It is placed once, when
    /// the submenu opens.
    rect: Rect,
    /// The submenu's pop-up window, and the part of the display that this
    /// window covers.
    window: Option<AnyWindowHandle>,
    bounds: Rect,
    serial: u64,
}

/// Says which panel a pop-up window shows: the menu's own panel, or the
/// submenu that has this serial number.
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
    /// The row that is under the pointer or was reached with the arrow keys.
    /// The pointer and the arrow keys move the same highlight.
    current: Option<usize>,
    highlights: motion::Fades,
    submenu: Option<Opened>,
    submenu_highlights: motion::Fades,
    submenu_grace: Option<Task<()>>,
    serial: u64,
    scroll: ScrollHandle,
    focus: FocusHandle,
    /// The time at which the root panel was first drawn in the window that
    /// shows it. The panel's opening animation starts then.
    shown: Option<Instant>,
    /// The time at which the menu was dismissed. From then the menu fades out
    /// for 80 ms (`motion::MENU_OUT`) and takes no more input.
    closing: Option<Instant>,
    /// The value of the row or swatch last chosen.
    chosen: Option<SharedString>,
    presentation: Presentation,
    /// The window the owner draws the menu in.
    main: Option<AnyWindowHandle>,
    /// The size of the owner's window. A menu that is drawn inside the window
    /// is kept inside this area.
    viewport: Rect,
    screen: Option<Screen>,
    opening: Option<Role>,
    /// The windows whose activation the menu watches, so that it closes when
    /// none of its windows has the keyboard.
    watched: Vec<AnyWindowHandle>,
    activation: Option<Task<()>>,
    close_windows: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Event> for Menu {}

impl Focusable for Menu {
    /// Returns the focus handle of the search field when the menu has one,
    /// and the menu's own focus handle otherwise.
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
        let search = (searchable >= SEARCH_THRESHOLD).then(|| {
            cx.new(|cx| {
                TextInput::new(crate::ui::text_input::Kind::Menu, cx).placeholder("Search...")
            })
        });
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

    /// Sets whether the menu may have a search row. With `false` it never
    /// has one. With `true` it has one when it has at least seven entries.
    pub fn search(mut self, show: bool) -> Self {
        if !show {
            self.search = None;
        }
        self
    }

    /// Makes the menu be drawn inside its owner's window and not in pop-up
    /// windows. A menu needs this when one of its entries holds a view with
    /// a text field, which needs the keyboard, or with a date field, whose
    /// calendar opens over the owner's window. A view that has only selects
    /// does not need it, because a select in a pop-up window opens its list
    /// in a pop-up window of its own.
    pub fn in_window(mut self) -> Self {
        self.presentation = Presentation::InWindow;
        self
    }

    /// Sets the panel's width, for a menu whose content needs a width other
    /// than the usual 270px.
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

    /// Replaces the menu's entries while the menu is open. A menu uses this
    /// when its rows show a state that choosing a row changes. The search
    /// text stays, and the highlight stays on the row at the same position.
    pub fn set_entries(&mut self, entries: Vec<Entry>, cx: &mut Context<Self>) {
        self.entries = entries;
        self.close_submenu(cx);
        cx.notify();
    }

    /// Replaces the top-level entry whose label is `label` with the entries
    /// in `replacement` while the menu is open. An empty `replacement`
    /// removes the entry. This is for an entry whose contents are known only
    /// after the menu has opened. A submenu that is open at that entry or
    /// below it is closed, because its row may have moved.
    pub fn replace_entry(&mut self, label: &str, replacement: Vec<Entry>, cx: &mut Context<Self>) {
        // The entries hold their labels translated, so `label` is translated
        // before it is compared with them.
        let label = crate::i18n::t(label);
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
            // The pop-up windows stay open while the menu fades out, and are
            // closed when the fade has ended.
            self.close_windows = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(motion::MENU_OUT).await;
                let _ = this.update(cx, |this, cx| this.close_pop_ups(cx));
            }));
        }
        cx.notify();
    }

    /// Returns the value of the row or swatch that was chosen last. Rows
    /// that share one action are told apart by this value ([`Item::value`],
    /// [`Swatch::value`]).
    pub fn chosen_value(&self) -> Option<SharedString> {
        self.chosen.clone()
    }

    /// Returns whether the keyboard focus is in the menu, in its search
    /// field, or in a field of an entry that holds a view.
    pub fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx) || self.focus_handle(cx).is_focused(window)
    }

    /// Returns whether the menu has finished fading out, so that its owner
    /// can drop it.
    pub fn finished(&self) -> bool {
        self.closing
            .is_some_and(|since| since.elapsed() >= motion::MENU_OUT)
    }

    /// Returns the scale and the opacity of the root panel at this moment.
    /// The scale is always 1. GPUI cannot scale the contents of an element,
    /// and a panel that grows while its contents stay the same size does
    /// not look like a panel that is scaled, so the panel never changes
    /// size. The opacity rises from 0 to 1 over `motion::MENU_IN` after the
    /// panel is first drawn, except in a pop-up window, which the system
    /// scales in as a whole (see `pop_in`). The opacity falls to 0 after the
    /// menu is dismissed. A submenu has no animation: an open submenu is
    /// drawn unchanged until the menu is dropped.
    fn root_motion(&self) -> (f32, f32) {
        let opacity = match (self.closing, self.shown) {
            (Some(since), _) => motion::menu_out(since.elapsed()).1,
            // The layer of the pop-up window is scaled in instead.
            (None, _) if self.presentation == Presentation::PopUpWindows => 1.,
            (None, Some(shown)) => {
                motion::EASE_SCALE_IN.ease(motion::progress(shown.elapsed(), motion::MENU_IN))
            }
            (None, None) => 0.,
        };
        (1., opacity)
    }

    /// Returns whether the root panel's opening animation is still running.
    fn entering(&self) -> bool {
        self.closing.is_none()
            && self
                .shown
                .is_none_or(|shown| shown.elapsed() < motion::MENU_IN)
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Returns the rows that the menu draws: all of its rows, or the rows
    /// that the search found while the search field has text.
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

    /// Returns, for each row, whether the arrow keys stop on it. They stop
    /// on an item that is not disabled and on a submenu's row.
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

    /// Returns the height the panel needs to show all of these rows.
    fn height(&self, rows: &[Row]) -> f32 {
        self.search_height() + items_height(rows, self.searching())
    }

    /// Returns the greatest height the panel can have: the height with
    /// every row, or the height with every row that a search can find,
    /// whichever is greater.
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

    /// Returns the area the menu is kept inside, and the position the menu
    /// was opened at in that area's coordinates.
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

    /// Returns the rectangle of the menu's panel. The panel is placed as if
    /// it had the full height of its rows, and its height is then limited
    /// to the area's height less 40px.
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

    /// Returns the point that the root panel is scaled about for these rows,
    /// as fractions of its width and height (`layout::origin`).
    fn root_origin(&self, rows: &[Row]) -> (f32, f32) {
        let (area, pointer) = self.area();
        origin(pointer, self.width, self.height(rows), area)
    }

    /// Returns the open submenu's row in the menu, the index of its entry,
    /// and its current row.
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

    /// Handles the right arrow key. When the current row's submenu is open,
    /// the highlight moves into the submenu. When the current row has a
    /// submenu that is closed, the submenu opens. Returns whether one of
    /// the two happened.
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

    /// Handles the left arrow key, which closes the open submenu. Returns
    /// whether a submenu was open.
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

    /// Handles a right click on a row. When the row has a context action,
    /// the menu emits that action to its owner and stays open.
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

    /// Called when the pointer moves onto a row that has no submenu. The
    /// open submenu closes after 150 ms (`motion::MENU_SUBMENU_GRACE`),
    /// unless the pointer reaches the submenu's panel first.
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

    /// Returns the width of a submenu's panel, which is the width that its
    /// widest row needs.
    fn submenu_width(&self, top: usize, window: &Window) -> f32 {
        let rows = self.submenu_rows(top);
        let has_icons = self.has_icons(&rows);
        let measure = |text: &SharedString, size: f32| {
            super::text_width(text, font(theme::FONT_UI), size, window)
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
    /// Picks a swatch. The menu emits the swatch row's action to its owner,
    /// and stays open with a ring drawn around the new choice.
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
