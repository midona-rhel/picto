//! The context menu (`ContextMenu.tsx`): a glass panel of rows at the pointer.
use crate::theme::Text as _;
use crate::{motion, theme};
use gpui::*;
use std::time::Instant;

actions!(menu, [Up, Down, Confirm, Cancel]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", Up, Some("Menu")),
        KeyBinding::new("down", Down, Some("Menu")),
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

pub enum Entry {
    Item(Item),
    Separator,
}

pub struct Item {
    /// What the owner is told was chosen.
    action: &'static str,
    label: SharedString,
    icon: Option<&'static str>,
    shortcut: Option<SharedString>,
    danger: bool,
    disabled: bool,
}

pub fn item(action: &'static str, label: impl Into<SharedString>) -> Item {
    Item {
        action,
        label: label.into(),
        icon: None,
        shortcut: None,
        danger: false,
        disabled: false,
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
}

impl From<Item> for Entry {
    fn from(item: Item) -> Self {
        Entry::Item(item)
    }
}

pub enum Event {
    /// A row was chosen; the menu is already closing.
    Chosen(&'static str),
}

pub struct Menu {
    entries: Vec<Entry>,
    position: Point<Pixels>,
    /// The row under the pointer or reached with the arrow keys: one
    /// highlight for both, as in the Electron menu.
    current: Option<usize>,
    highlights: motion::Fades,
    focus: FocusHandle,
    /// Set once dismissed: the menu fades for 80 ms and takes no more input.
    closing: Option<Instant>,
}

impl EventEmitter<Event> for Menu {}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Menu {
    pub fn new(entries: Vec<Entry>, position: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        Self {
            entries,
            position,
            current: None,
            highlights: motion::Fades::default(),
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

    fn actionable(&self, index: usize) -> bool {
        matches!(&self.entries[index], Entry::Item(item) if !item.disabled)
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

    /// Moves to the next row that can be chosen, wrapping at the ends.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let rows = (0..self.entries.len())
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

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.closing.is_some() || !self.actionable(index) {
            return;
        }
        if let Entry::Item(item) = &self.entries[index] {
            cx.emit(Event::Chosen(item.action));
        }
        self.dismiss(cx);
    }

    fn height(&self) -> f32 {
        let rows = self
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Item(_) => theme::CONTROL_SM,
                Entry::Separator => SEPARATOR,
            })
            .sum::<f32>();
        // The panel's border sits outside this, as a 1px ring.
        2. * PADDING + rows + GAP * self.entries.len().saturating_sub(1) as f32
    }

    fn row(&self, index: usize, item: &Item, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let current = self.current == Some(index);
        let color = if item.danger {
            theme.negative()
        } else {
            theme.text()
        };
        let mut row = div()
            .id(("menu-row", index))
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
                    .bg(if item.danger {
                        theme.negative_at(12.)
                    } else {
                        theme.hover_overlay()
                    })
                    .opacity(self.highlights.value(("row", index))),
            );
        if let Some(icon) = item.icon {
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
        row = row.child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(item.label.clone()),
        );
        if let Some(shortcut) = &item.shortcut {
            row = row.child(
                div()
                    .relative()
                    .flex_none()
                    .pl(px(28.))
                    .text(theme::TEXT_CAPTION)
                    .opacity(0.5)
                    .child(shortcut.clone()),
            );
        }
        if item.disabled {
            return row.opacity(0.4);
        }
        row.cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.set_current(Some(index), cx);
                } else if this.current == Some(index) {
                    this.set_current(None, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
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
            .p(px(PADDING))
            .gap(px(GAP))
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            .opacity(opacity)
            // A press on the panel itself is not a press outside it.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            // `--glass-border-side`, drawn inside so it takes no room.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_LG))
                    .border_1()
                    .border_color(theme.glass_border()),
            );
        for (index, entry) in self.entries.iter().enumerate() {
            panel = match entry {
                Entry::Item(item) => panel.child(self.row(index, item, cx)),
                Entry::Separator => panel.child(
                    div()
                        .relative()
                        .h(px(1.))
                        .my(px(2.))
                        .flex_none()
                        .bg(theme.border_secondary()),
                ),
            };
        }
        let mut layer = div()
            .id("menu-layer")
            .absolute()
            .inset_0()
            .track_focus(&self.focus)
            .key_context("Menu")
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(true, cx)))
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
        layer.child(panel)
    }
}
