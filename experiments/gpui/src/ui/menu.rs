//! The context menu (`ContextMenu.tsx`): a glass panel of rows at the pointer.
use super::text_input::{Submitted, TextInput};
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::{prelude::FluentBuilder as _, *};
use std::time::Instant;

actions!(menu, [Up, Down, Left, Right, Confirm, Cancel]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", Up, Some("Menu")),
        KeyBinding::new("down", Down, Some("Menu")),
        KeyBinding::new("left", Left, Some("Menu")),
        KeyBinding::new("right", Right, Some("Menu")),
        KeyBinding::new("enter", Confirm, Some("Menu")),
        KeyBinding::new("escape", Cancel, Some("Menu")),
    ]);
}

const WIDTH: f32 = 270.;
/// The panel keeps this far from the window's edges.
const MARGIN: f32 = 12.;
const PADDING: f32 = 4.;
const GAP: f32 = 2.;
/// A separator's line and its 2px margins.
const SEPARATOR: f32 = 5.;
/// `ColorPicker`'s row: 18px swatches with 4px above and below.
const SWATCH_ROW: f32 = 26.;
const SWATCH: f32 = 18.;
/// A submenu opens this far from the menu's edge, its first row level with
/// the row that opened it.
const SUBMENU_GAP: f32 = 4.;
const SUBMENU_RISE: f32 = 3.;
/// `SEARCH_ITEM_THRESHOLD`: a menu with this many rows to choose from opens
/// with a search field.
const SEARCH_THRESHOLD: usize = 7;
/// `.searchArea`
const SEARCH_HEIGHT: f32 = 36.;

pub enum Entry {
    Item(Item),
    Separator,
    /// A row that opens a panel of further rows beside the menu.
    Submenu(Submenu),
    /// A row of colour swatches (`ColorPicker`).
    Swatches(Swatches),
}

pub struct Item {
    /// What the owner is told was chosen.
    action: &'static str,
    label: SharedString,
    icon: Option<&'static str>,
    shortcut: Option<SharedString>,
    danger: bool,
    disabled: bool,
    /// Which of several rows with the same action this is.
    value: Option<SharedString>,
}

pub fn item(action: &'static str, label: impl Into<SharedString>) -> Item {
    Item {
        action,
        label: label.into(),
        icon: None,
        shortcut: None,
        danger: false,
        disabled: false,
        value: None,
    }
}

impl Item {
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    /// The shortcut as the platform writes it, e.g. `⌘⌫`.
    pub fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut.into());
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
    /// Tells rows that share an action apart: the owner reads it with
    /// [`Menu::chosen_value`].
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
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
    children: Vec<Item>,
}

pub fn submenu(label: impl Into<SharedString>, children: Vec<Item>) -> Submenu {
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
    /// `None` is the default, drawn in the primary text colour.
    pub color: Option<Rgba>,
    /// What [`Menu::chosen_value`] gives when it is picked.
    pub value: SharedString,
}

pub struct Swatches {
    action: &'static str,
    swatches: Vec<Swatch>,
    selected: Option<usize>,
}

/// A row of colour swatches. Picking one tells the owner and leaves the
/// menu open, with the new choice ringed.
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

pub enum Event {
    /// A row was chosen. A swatch leaves the menu open; any other row has
    /// already started it closing.
    Chosen(&'static str),
}

/// A row the menu shows: one of its entries, or, while searching, a row of
/// a submenu brought up to the top level.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Entry(usize),
    Child(usize, usize),
}

/// `menuSearchScore`: lower is better; none when the label does not match.
fn search_score(label: &str, query: &str) -> Option<usize> {
    if label.starts_with(query) {
        return Some(0);
    }
    if let Some(index) = label.find(query) {
        return Some(10 + index);
    }
    // The label followed by its (absent) keywords.
    let text = format!("{label} ");
    if let Some(index) = text.find(query) {
        return Some(100 + index);
    }
    // The query's letters in order, with what lies between them counted.
    let wanted = query.chars().collect::<Vec<_>>();
    let (mut found, mut gap) = (0, 0);
    for character in text.chars() {
        if found == wanted.len() {
            break;
        }
        if character == wanted[found] {
            found += 1;
        } else if found > 0 {
            gap += 1;
        }
    }
    (found == wanted.len()).then_some(200 + gap)
}

/// The rows to show for `query`: every entry when it is blank, else
/// `searchMenuEntries`: the rows and submenu rows that match, best first,
/// without separators or swatches.
fn shown_for(entries: &[Entry], query: &str) -> Vec<Slot> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return (0..entries.len()).map(Slot::Entry).collect();
    }
    let mut matches = Vec::new();
    let mut order = 0;
    let mut consider = |slot: Slot, label: &SharedString, disabled: bool| {
        order += 1;
        if !disabled && let Some(score) = search_score(&label.to_lowercase(), &query) {
            matches.push((score, order, slot));
        }
    };
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            Entry::Item(item) => consider(Slot::Entry(index), &item.label, item.disabled),
            Entry::Submenu(submenu) => {
                consider(Slot::Entry(index), &submenu.label, false);
                for (child, item) in submenu.children.iter().enumerate() {
                    consider(Slot::Child(index, child), &item.label, item.disabled);
                }
            }
            _ => {}
        }
    }
    // Entries are visited in order, so the order they were found in breaks
    // ties between equal scores.
    matches.sort_by_key(|(score, order, _)| (*score, *order));
    matches.into_iter().map(|(_, _, slot)| slot).collect()
}

pub struct Menu {
    entries: Vec<Entry>,
    /// The rows showing, which the search narrows; the positions below are
    /// positions in this list.
    shown: Vec<Slot>,
    position: Point<Pixels>,
    /// The row under the pointer or reached with the arrow keys: one
    /// highlight for both, as in the Electron menu.
    current: Option<usize>,
    highlights: motion::Fades,
    /// The submenu showing, by the position of the row that opened it.
    open_submenu: Option<usize>,
    /// The submenu row under the pointer.
    sub_current: Option<usize>,
    /// Closes the submenu once the pointer has been off it for 150 ms.
    submenu_timer: Option<Task<()>>,
    /// The value of the last row or swatch chosen.
    chosen: Option<SharedString>,
    focus: FocusHandle,
    /// Set once dismissed: the menu fades for 80 ms and takes no more input.
    closing: Option<Instant>,
    /// The search field of a long menu, and what it holds.
    search: Option<Entity<TextInput>>,
    query: String,
    _search: Vec<Subscription>,
}

impl EventEmitter<Event> for Menu {}

impl Focusable for Menu {
    /// The search field when the menu has one, as it takes the keyboard.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.search {
            Some(search) => search.read(cx).focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

/// Where a submenu panel goes: right of the menu, or left of it when there
/// is no room on the right, and moved up or down to keep inside the window.
fn submenu_origin(
    menu_left: f32,
    row_top: f32,
    width: f32,
    height: f32,
    view: Size<Pixels>,
) -> (f32, f32) {
    let (view_width, view_height) = (f32::from(view.width), f32::from(view.height));
    let mut left = menu_left + WIDTH + SUBMENU_GAP;
    if left + width > view_width - MARGIN {
        left = menu_left - width - SUBMENU_GAP;
    }
    let mut top = row_top - SUBMENU_RISE;
    if top + height > view_height - MARGIN {
        top = view_height - height - MARGIN;
    }
    (left, top.max(MARGIN))
}

impl Menu {
    pub fn new(entries: Vec<Entry>, position: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        let searchable = entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Item(_) | Entry::Submenu(_)))
            .count();
        let (search, subscriptions) = if searchable >= SEARCH_THRESHOLD {
            let search = cx.new(|cx| TextInput::plain("Search...", cx));
            let subscriptions = vec![
                cx.observe(&search, |this, search, cx| {
                    let query = search.read(cx).value();
                    this.filter(query, cx);
                }),
                // Enter in the field chooses the highlighted row.
                cx.subscribe(&search, |this, _, _: &Submitted, cx| {
                    if let Some(current) = this.current {
                        this.choose(current, cx);
                    }
                }),
            ];
            (Some(search), subscriptions)
        } else {
            (None, Vec::new())
        };
        Self {
            shown: shown_for(&entries, ""),
            search,
            query: String::new(),
            _search: subscriptions,
            entries,
            position,
            current: None,
            highlights: motion::Fades::default(),
            open_submenu: None,
            sub_current: None,
            submenu_timer: None,
            chosen: None,
            focus: cx.focus_handle(),
            closing: None,
        }
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.closing.is_none() {
            self.closing = Some(Instant::now());
            cx.notify();
        }
    }

    /// Whether the menu has faded out and can be dropped.
    pub fn finished(&self) -> bool {
        self.closing
            .is_some_and(|since| since.elapsed() >= motion::MENU_OUT)
    }

    /// The value of the row or swatch last chosen, for rows that share an
    /// action.
    pub fn chosen_value(&self) -> Option<SharedString> {
        self.chosen.clone()
    }

    /// Narrows the rows to those matching `query`. The highlight and any
    /// open submenu start again.
    fn filter(&mut self, query: String, cx: &mut Context<Self>) {
        if query == self.query {
            return;
        }
        self.shown = shown_for(&self.entries, &query);
        self.query = query;
        self.current = None;
        self.open_submenu = None;
        self.sub_current = None;
        self.submenu_timer = None;
        self.highlights = motion::Fades::default();
        cx.notify();
    }

    /// The item a row chooses, if it is an item.
    fn item_at(&self, position: usize) -> Option<&Item> {
        match *self.shown.get(position)? {
            Slot::Entry(index) => match &self.entries[index] {
                Entry::Item(item) => Some(item),
                _ => None,
            },
            Slot::Child(parent, child) => match &self.entries[parent] {
                Entry::Submenu(submenu) => submenu.children.get(child),
                _ => None,
            },
        }
    }

    /// The submenu a row opens, if it opens one.
    fn submenu_at(&self, position: usize) -> Option<(usize, &Submenu)> {
        match *self.shown.get(position)? {
            Slot::Entry(index) => match &self.entries[index] {
                Entry::Submenu(submenu) => Some((index, submenu)),
                _ => None,
            },
            Slot::Child(..) => None,
        }
    }

    fn actionable(&self, position: usize) -> bool {
        self.item_at(position).is_some_and(|item| !item.disabled)
            || self.submenu_at(position).is_some()
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

    fn set_sub_current(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        if self.sub_current == row {
            return;
        }
        if let Some(previous) = self.sub_current {
            self.highlights
                .set(("sub", previous), false, motion::MENU_ROW);
        }
        if let Some(next) = row {
            self.highlights.set(("sub", next), true, motion::MENU_ROW);
        }
        self.sub_current = row;
        cx.notify();
    }

    /// The pointer has come to a row: a submenu row opens its panel at once;
    /// any other row lets an open one close after 150 ms.
    fn submenu_intent(&mut self, target: Option<usize>, cx: &mut Context<Self>) {
        self.submenu_timer = None;
        match target {
            Some(index) => self.show_submenu(Some(index), cx),
            None if self.open_submenu.is_some() => {
                self.submenu_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(motion::SUBMENU_CLOSE).await;
                    let _ = this.update(cx, |this: &mut Self, cx| {
                        this.show_submenu(None, cx);
                    });
                }));
            }
            None => {}
        }
    }

    fn show_submenu(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.open_submenu != index {
            self.open_submenu = index;
            self.set_sub_current(None, cx);
            cx.notify();
        }
    }

    /// Moves to the next row that can be chosen, wrapping at the ends.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let rows = (0..self.shown.len())
            .filter(|index| self.actionable(*index))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return;
        }
        let at = self
            .current
            .and_then(|current| rows.iter().position(|row| *row == current));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % rows.len(),
            (Some(at), false) => (at + rows.len() - 1) % rows.len(),
            (None, true) => 0,
            (None, false) => rows.len() - 1,
        };
        self.set_current(Some(rows[next]), cx);
    }

    fn choose(&mut self, position: usize, cx: &mut Context<Self>) {
        if self.closing.is_some() || !self.actionable(position) {
            return;
        }
        if let Some(item) = self.item_at(position) {
            let action = item.action;
            self.chosen = item.value.clone();
            cx.emit(Event::Chosen(action));
            self.dismiss(cx);
        } else {
            // Enter on a submenu row opens or closes its panel.
            let target = (self.open_submenu != Some(position)).then_some(position);
            self.submenu_timer = None;
            self.show_submenu(target, cx);
        }
    }

    fn choose_child(&mut self, parent: usize, child: usize, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        if let Some(Entry::Submenu(submenu)) = self.entries.get(parent)
            && let Some(item) = submenu.children.get(child).filter(|item| !item.disabled)
        {
            self.chosen = item.value.clone();
            cx.emit(Event::Chosen(item.action));
            self.dismiss(cx);
        }
    }

    fn pick_swatch(&mut self, index: usize, swatch: usize, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        if let Some(Entry::Swatches(row)) = self.entries.get_mut(index)
            && let Some(choice) = row.swatches.get(swatch)
        {
            row.selected = Some(swatch);
            self.chosen = Some(choice.value.clone());
            cx.emit(Event::Chosen(row.action));
            cx.notify();
        }
    }

    fn entry_height(entry: &Entry) -> f32 {
        match entry {
            Entry::Item(_) | Entry::Submenu(_) => theme::CONTROL_SM,
            Entry::Separator => SEPARATOR,
            Entry::Swatches(_) => SWATCH_ROW,
        }
    }

    fn slot_height(&self, slot: Slot) -> f32 {
        match slot {
            Slot::Entry(index) => Self::entry_height(&self.entries[index]),
            Slot::Child(..) => theme::CONTROL_SM,
        }
    }

    /// The search field's height, when the menu has one.
    fn search_height(&self) -> f32 {
        if self.search.is_some() {
            SEARCH_HEIGHT
        } else {
            0.
        }
    }

    fn height(&self) -> f32 {
        let rows = if self.shown.is_empty() {
            // "No results"
            2. * 8. + theme::TEXT_SM + 2.
        } else {
            self.shown
                .iter()
                .map(|slot| self.slot_height(*slot))
                .sum::<f32>()
                + GAP * (self.shown.len() - 1) as f32
        };
        // The panel's border sits outside this, as a 1px ring.
        self.search_height() + 2. * PADDING + rows
    }

    /// How far below the panel's top the row at `position` starts.
    fn row_offset(&self, position: usize) -> f32 {
        self.search_height()
            + PADDING
            + self.shown[..position]
                .iter()
                .map(|slot| self.slot_height(*slot) + GAP)
                .sum::<f32>()
    }

    /// The drawing of a row, without what it does: its highlight, glyph,
    /// label and shortcut or chevron.
    #[allow(clippy::too_many_arguments)]
    fn row_frame(
        id: ElementId,
        label: SharedString,
        icon: Option<&'static str>,
        trailing: Option<AnyElement>,
        danger: bool,
        highlight: f32,
        current: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let color = if danger {
            theme.negative()
        } else {
            theme.text()
        };
        let mut row = div()
            .id(id)
            .relative()
            .h(px(theme::CONTROL_SM))
            .flex_none()
            .px(px(6.))
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
        if let Some(icon) = icon {
            row = row.child(
                div()
                    .relative()
                    .w(px(16.))
                    .mr(px(6.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(super::icon(icon, 16., color).opacity(if current { 1. } else { 0.8 })),
            );
        }
        row.child(div().relative().flex_1().min_w_0().truncate().child(label))
            .children(trailing)
    }

    fn row(&self, index: usize, item: &Item, cx: &mut Context<Self>) -> Stateful<Div> {
        let current = self.current == Some(index);
        let shortcut = item.shortcut.clone().map(|shortcut| {
            div()
                .relative()
                .flex_none()
                .pl(px(28.))
                .text(theme::TEXT_CAPTION)
                .opacity(0.5)
                .child(shortcut)
                .into_any_element()
        });
        let row = Self::row_frame(
            ("menu-row", index).into(),
            item.label.clone(),
            item.icon,
            shortcut,
            item.danger,
            self.highlights.value(("row", index)),
            current,
            cx,
        );
        if item.disabled {
            return row.opacity(0.4);
        }
        row.cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.set_current(Some(index), cx);
                    this.submenu_intent(None, cx);
                } else if this.current == Some(index) {
                    this.set_current(None, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
    }

    fn submenu_row(
        &self,
        index: usize,
        submenu: &Submenu,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let chevron = div()
            .relative()
            .flex_none()
            .ml_auto()
            .child(super::icon("menu-chevron-right", 12., theme.text()).opacity(0.4))
            .into_any_element();
        Self::row_frame(
            ("menu-row", index).into(),
            submenu.label.clone(),
            submenu.icon,
            Some(chevron),
            false,
            self.highlights.value(("row", index)),
            self.current == Some(index),
            cx,
        )
        .cursor_pointer()
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.set_current(Some(index), cx);
                this.submenu_intent(Some(index), cx);
            } else if this.current == Some(index) {
                this.set_current(None, cx);
            }
        }))
        .on_click(cx.listener(move |this, _, _, cx| {
            let target = (this.open_submenu != Some(index)).then_some(index);
            this.submenu_timer = None;
            this.show_submenu(target, cx);
        }))
    }

    /// `ColorPicker`: swatches spread across the row, the chosen one ringed
    /// in its colour over the app background, with a check.
    fn swatch_row(
        &self,
        index: usize,
        row: &Swatches,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let mut line = div()
            .id(("menu-swatches", index))
            .relative()
            .h(px(SWATCH_ROW))
            .flex_none()
            .px(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.set_current(None, cx);
                    this.submenu_intent(None, cx);
                }
            }));
        for (position, swatch) in row.swatches.iter().enumerate() {
            let color = swatch.color.unwrap_or(theme.text());
            let selected = row.selected == Some(position);
            let mut dot = div()
                .size(px(SWATCH))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(color);
            if selected {
                let check = if swatch.color.is_some() {
                    rgb(0xffffff)
                } else {
                    theme.background()
                };
                dot = dot.child(super::icon("swatch-check", 10., check));
            }
            // `box-shadow: 0 0 0 1.5px var(--color-bg-app), 0 0 0 3px <colour>`
            let ring = div()
                .size(px(SWATCH))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .when(selected, |ring| {
                    ring.shadow(vec![
                        BoxShadow::new(px(0.), px(0.), theme.background().into())
                            .spread_radius(px(1.5)),
                        BoxShadow::new(px(0.), px(0.), color.into()).spread_radius(px(3.)),
                    ])
                })
                .child(dot);
            let target = div()
                .id(("menu-swatch", position))
                .cursor_pointer()
                .child(ring)
                .on_click(cx.listener(move |this, _, _, cx| this.pick_swatch(index, position, cx)));
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

    /// The width a submenu panel needs for its widest row.
    fn submenu_width(submenu: &Submenu, window: &mut Window) -> f32 {
        let mut style = window.text_style();
        style.font_family = theme::FONT_UI.into();
        let has_icons = submenu.children.iter().any(|item| item.icon.is_some());
        let widest = submenu
            .children
            .iter()
            .map(|item| {
                let run = style.to_run(item.label.len());
                f32::from(
                    window
                        .text_system()
                        .shape_line(item.label.clone(), px(theme::TEXT_MD), &[run], None)
                        .width,
                )
                .ceil()
            })
            .fold(0., f32::max);
        2. * PADDING + 2. * 6. + if has_icons { 22. } else { 0. } + widest
    }

    fn submenu_panel(
        &self,
        parent: usize,
        submenu: &Submenu,
        origin: (f32, f32),
        width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let mut panel = div()
            .id("submenu")
            .occlude()
            .absolute()
            .left(px(origin.0))
            .top(px(origin.1))
            .w(px(width))
            .p(px(PADDING))
            .gap(px(GAP))
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            // The pointer reaching the panel keeps it open.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.submenu_timer = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_LG))
                    .border_1()
                    .border_color(theme.glass_border()),
            );
        for (child, item) in submenu.children.iter().enumerate() {
            let row = Self::row_frame(
                ("submenu-row", child).into(),
                item.label.clone(),
                item.icon,
                None,
                item.danger,
                self.highlights.value(("sub", child)),
                self.sub_current == Some(child),
                cx,
            );
            panel = panel.child(if item.disabled {
                row.opacity(0.4)
            } else {
                row.cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_sub_current(Some(child), cx);
                        } else if this.sub_current == Some(child) {
                            this.set_sub_current(None, cx);
                        }
                    }))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.choose_child(parent, child, cx)),
                    )
            });
        }
        panel
    }
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let opacity = match self.closing {
            Some(since) => {
                1. - motion::EASE_IN.ease(motion::progress(since.elapsed(), motion::MENU_OUT))
            }
            None => 1.,
        };
        if self.closing.is_some() || self.highlights.animating() {
            window.request_animation_frame();
        }
        // The panel opens with its corner at the pointer, moved inward so it
        // keeps a margin from every edge of the window.
        let view = window.viewport_size();
        let height = self.height();
        let left = f32::from(self.position.x)
            .min(f32::from(view.width) - WIDTH - MARGIN)
            .max(MARGIN);
        let top = f32::from(self.position.y)
            .min(f32::from(view.height) - height - MARGIN)
            .max(MARGIN);

        let mut panel = div()
            .id("menu")
            .occlude()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            // A press on the panel itself is not a press outside it.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation());
        if let Some(search) = &self.search {
            // `.searchArea`, with its line below.
            panel = panel.child(
                div()
                    .relative()
                    .h(px(SEARCH_HEIGHT))
                    .flex_none()
                    .child(
                        super::field("menu-search", search, super::field::Look::MenuSearch).icon(
                            "menu-search-field",
                            16.,
                            10.,
                            theme::blend(
                                gpui::transparent_black().into(),
                                theme.text_tertiary(),
                                0.6,
                            ),
                        ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.))
                            .bg(theme.border_secondary()),
                    ),
            );
        }
        let mut items = div()
            .relative()
            .p(px(PADDING))
            .gap(px(GAP))
            .flex()
            .flex_col();
        let mut submenu = None;
        for (position, slot) in self.shown.iter().copied().enumerate() {
            let (index, entry) = match slot {
                Slot::Entry(index) => (index, &self.entries[index]),
                Slot::Child(..) => {
                    if let Some(item) = self.item_at(position) {
                        items = items.child(self.row(position, item, cx));
                    }
                    continue;
                }
            };
            items = match entry {
                Entry::Item(item) => items.child(self.row(position, item, cx)),
                Entry::Submenu(child) => {
                    if self.open_submenu == Some(position) {
                        let width = Self::submenu_width(child, window);
                        let rows = child.children.len() as f32;
                        let height =
                            2. * PADDING + rows * theme::CONTROL_SM + GAP * (rows - 1.).max(0.);
                        let origin = submenu_origin(
                            left,
                            top + self.row_offset(position),
                            width,
                            height,
                            view,
                        );
                        submenu = Some(self.submenu_panel(index, child, origin, width, cx));
                    }
                    items.child(self.submenu_row(position, child, cx))
                }
                Entry::Swatches(row) => items.child(self.swatch_row(index, row, window, cx)),
                Entry::Separator => items.child(
                    div()
                        .relative()
                        .h(px(1.))
                        .my(px(2.))
                        .flex_none()
                        .bg(theme.border_secondary()),
                ),
            };
        }
        if self.shown.is_empty() {
            // `.empty`
            items = items.child(
                div()
                    .py(px(8.))
                    .px(px(12.))
                    .flex()
                    .justify_center()
                    .text_color(theme.text_tertiary())
                    .text(theme::TEXT_SM)
                    .child("No results"),
            );
        }
        panel = panel.child(items).child(
            // `--glass-border-side`, drawn inside so it takes no room.
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_LG))
                .border_1()
                .border_color(theme.glass_border()),
        );
        let mut layer = div()
            .id("menu-layer")
            .absolute()
            .inset_0()
            .opacity(opacity)
            .track_focus(&self.focus)
            .key_context("Menu")
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| {
                if let Some(current) = this
                    .current
                    .filter(|current| this.submenu_at(*current).is_some())
                {
                    this.submenu_timer = None;
                    this.show_submenu(Some(current), cx);
                } else {
                    // Only a submenu row takes the arrow.
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Left, _, cx| {
                if this.open_submenu.is_some() {
                    this.show_submenu(None, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| {
                if let Some(current) = this.current {
                    this.choose(current, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.dismiss(cx)));
        // While open, the layer takes every press outside the panel, with
        // any button, and nothing beneath it sees the pointer.
        if self.closing.is_none() {
            layer = layer
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _, _, cx| {
                    this.dismiss(cx);
                    cx.stop_propagation();
                }));
        }
        layer.child(panel).children(submenu)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, MARGIN, Slot, WIDTH, item, search_score, shown_for, submenu, submenu_origin,
    };
    use gpui::{px, size};

    #[test]
    fn a_search_finds_rows_and_submenu_rows_best_first() {
        let entries: Vec<Entry> = vec![
            item("copy", "Copy Tag").into(),
            Entry::Separator,
            submenu(
                "Move to Group…",
                vec![item("move", "creator"), item("move", "series")],
            )
            .into(),
            item("delete", "Delete Tag").into(),
            item("off", "Tag Off").disabled(true).into(),
        ];
        // A blank search shows every entry, separators included.
        assert_eq!(shown_for(&entries, "  ").len(), 5);
        // Label prefix, then a match inside a label, in menu order; a
        // disabled row is never offered.
        assert_eq!(shown_for(&entries, "tag"), [Slot::Entry(0), Slot::Entry(3)]);
        assert_eq!(shown_for(&entries, "ser"), [Slot::Child(2, 1)]);
        assert_eq!(shown_for(&entries, "zzz"), []);
        assert_eq!(search_score("delete tag", "delete"), Some(0));
        assert_eq!(search_score("copy tag", "tag"), Some(15));
        assert_eq!(search_score("delete tag", "dt"), Some(203));
    }

    #[test]
    fn a_submenu_opens_beside_the_menu_and_stays_inside_the_window() {
        let view = size(px(1000.), px(600.));
        // Room on the right: 4px from the menu, 3px above its row.
        assert_eq!(
            submenu_origin(100., 200., 150., 100., view),
            (100. + WIDTH + 4., 197.)
        );
        // No room on the right: it opens on the left instead.
        assert_eq!(submenu_origin(700., 200., 150., 100., view).0, 700. - 154.);
        // Too low: it rises to keep the margin; too high: it drops to it.
        assert_eq!(
            submenu_origin(100., 560., 150., 100., view).1,
            600. - 100. - MARGIN
        );
        assert_eq!(submenu_origin(100., 5., 150., 100., view).1, MARGIN);
    }
}
