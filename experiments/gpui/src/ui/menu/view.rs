//! The menu's drawing: its keys, the panels' chrome, rows, swatch rows,
//! view entries and separators, in the owner's window or a pop-up window.
use super::layout::{Path, Rect, Row};
use super::{
    CHECK_SLOT, CUSTOM_PADDING, Cancel, CloseSubmenu, Confirm, Down, Entry, GAP, Menu, OpenSubmenu,
    Opened, PADDING, Presentation, ROW_PADDING, Role, SEARCH_HEIGHT, SEARCH_INPUT_LEFT,
    SHORTCUT_GAP, SWATCH, SWATCH_ROW, Up,
};
use crate::theme::Text as _;
use crate::ui::text_input;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::time::Instant;

impl Menu {
    /// The menu's keys, on whichever of its windows has the keyboard. The
    /// search field binds Enter, Escape and the side arrows itself; they are
    /// taken here before it sees them.
    fn keyboard(&self, element: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        // `role="menu"`: the element with the keyboard, whose highlighted
        // row assistive technology reads as focused.
        element
            .role(gpui::Role::Menu)
            .aria_label(crate::i18n::t("Context menu"))
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
        window: &Window,
        cx: &App,
    ) -> crate::ui::GlassPanel {
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
            .when(self.presentation == Presentation::PopUpWindows, |panel| {
                // What is behind is another window: the window's system blur
                // shows beneath the panel instead, under a fill that gives it
                // the glass's brightness, or, where the system cannot blur
                // there, a more opaque glass.
                let theme = theme::theme(cx);
                let fill = if crate::ui::window_chrome::system_blurs() {
                    crate::ui::window_chrome::place_system_blur(
                        window,
                        Bounds::new(
                            point(px(at.0 + dx), px(at.1 + dy)),
                            size(px(scaled_w), px(scaled_h)),
                        ),
                        theme::RADIUS_LG,
                        opacity,
                    );
                    crate::ui::fill_over_system_blur(theme.glass(), theme.glass_blur().brightness)
                } else {
                    crate::ui::fill_without_blur(theme.glass())
                };
                panel.without_backdrop_blur().bg(fill)
            })
    }

    /// A row as `.item` draws it, without what it does.
    fn row(
        &self,
        id: impl Into<ElementId>,
        entry: &Entry,
        highlight: f32,
        current: bool,
        has_icons: bool,
        state: RowState,
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
        // `role="menuitem"` (a check row as a checkable item, a submenu's
        // row expanded while its submenu shows), named by its label.
        let mut row = div()
            .id(id)
            .role(match item {
                Some(item) if item.checked.is_some() || item.excluded => {
                    gpui::Role::MenuItemCheckBox
                }
                _ => gpui::Role::MenuItem,
            })
            .aria_label(label.clone())
            .aria_disabled(item.is_some_and(|item| item.disabled))
            .when(state.active, |row| row.aria_active_descendant())
            .when_some(state.expanded, |row, open| row.aria_expanded(open))
            .when_some(item.and_then(|item| item.checked), |row, checked| {
                row.aria_toggled(crate::ui::a11y::toggled(checked))
            })
            .when_some(item.and_then(|item| item.shortcut.clone()), |row, keys| {
                row.aria_keyshortcuts(keys)
            })
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
                        super::icon(icon, 16., color)
                            .opacity(if current { 1. } else { 0.8 })
                            .into_any_element()
                    }))
                    .children(item.and_then(|item| item.image.clone()).map(|image| {
                        img(image)
                            .size(px(16.))
                            .flex_none()
                            .opacity(if current { 1. } else { 0.8 })
                            .into_any_element()
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
    ) -> crate::ui::GlassPanel {
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
            // While a row of the open submenu is current, that row is the
            // one assistive technology reads as focused.
            let in_submenu = self
                .submenu
                .as_ref()
                .is_some_and(|open| open.current.is_some());
            let state = RowState {
                active: self.current == Some(index) && !in_submenu,
                expanded: matches!(entry, Entry::Submenu(_))
                    .then(|| self.submenu.as_ref().is_some_and(|open| open.row == index)),
            };
            let element = self.row(
                ("menu-row", index),
                entry,
                self.highlights.value(("row", index)),
                self.current == Some(index),
                has_icons,
                state,
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
                    .child(crate::i18n::t("No results")),
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
            window,
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
    ) -> crate::ui::GlassPanel {
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
                RowState {
                    active: open.current == Some(index),
                    expanded: None,
                },
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
                .role(gpui::Role::Menu)
                .aria_label(crate::i18n::t("Context submenu"))
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
            window,
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
            let ring = super::color_picker::swatch(color, selected.then_some(check), cx);
            let target = div()
                .id(("menu-swatch", position))
                .role(gpui::Role::MenuItemRadio)
                .aria_label(swatch.label.clone())
                .aria_toggled(crate::ui::a11y::toggled(selected))
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

/// What assistive technology is told about a row beyond its entry.
#[derive(Clone, Copy)]
struct RowState {
    /// The row is the one the keyboard is on (`aria-activedescendant`).
    active: bool,
    /// A submenu's row: whether its submenu shows.
    expanded: Option<bool>,
}

fn separator(cx: &App) -> Div {
    div()
        .relative()
        .h(px(1.))
        .my(px(2.))
        .flex_none()
        .bg(theme::theme(cx).border_secondary())
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
