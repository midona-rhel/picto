//! The Duplicates screen's drawing: its loading, empty and error states,
//! the view the mode chooses, and the titlebar controls.
use super::{
    ActualPixels, CONTEXT, DuplicatesScreen, DuplicatesToolbar, FitPair, KeepAll, Leave, Merge,
    Mode, Next, NotDuplicate, OpenCompare, Previous, SetAbove, SetBelow, Sort, Tab,
};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::*;

impl DuplicatesScreen {
    /// The first read, with its message after 200 ms.
    fn loading_state(&self, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text(theme::TEXT_SM)
            .text_color(theme.text_secondary())
            .children(
                self.loading_message
                    .then_some(t("Loading duplicate review queue...")),
            )
    }

    /// Nothing to review (Eagle's words), or the read failed; with the
    /// scan's button and its progress bar's place.
    fn empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let (title, description) = match &self.error {
            Some(error) => (
                t("Unable to load duplicate review").to_owned(),
                error.clone(),
            ),
            None => (
                en("Enjoy your day").to_owned(),
                en("You have no duplicated items.").to_owned(),
            ),
        };
        let action = ui::empty_state_action(
            "duplicates-scan",
            "action-refresh",
            if self.error.is_some() {
                t("Retry")
            } else {
                t("Scan library")
            },
        )
        .disabled(self.scanning || self.busy)
        .on_click(cx.listener(|this, _, _, cx| {
            if this.error.is_some() && !this.scanning {
                this.error = None;
                this.fetch(cx);
            } else {
                this.scan(cx);
            }
        }));
        ui::empty_state(
            "empty-copy",
            title,
            description,
            vec![action.into_any_element()],
            cx,
        )
        // The 180 × 2 slot of the scan's progress bar, kept while empty.
        .child(
            div()
                .w(px(180.))
                .h(px(2.))
                .flex_none()
                .overflow_hidden()
                .children(self.scan_progress.then(|| ui::progress_bar().height(2.))),
        )
        .into_any_element()
    }
}

impl Render for DuplicatesScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let mut root = div()
            .id("duplicates")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Next, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SetAbove, _, cx| this.step_row(-1, cx)))
            .on_action(cx.listener(|this, _: &SetBelow, _, cx| this.step_row(1, cx)))
            .on_action(cx.listener(|this, _: &Merge, _, cx| this.merge_key(cx)))
            .on_action(cx.listener(|this, _: &OpenCompare, _, cx| this.compare_key(cx)))
            .on_action(cx.listener(|this, _: &KeepAll, _, cx| this.apart_key(cx)))
            .on_action(cx.listener(|this, _: &NotDuplicate, _, cx| this.apart_key(cx)))
            .on_action(cx.listener(|this, _: &FitPair, _, cx| this.fit(cx)))
            .on_action(cx.listener(|this, _: &ActualPixels, _, cx| this.actual(cx)))
            .on_action(cx.listener(|this, _: &Leave, _, cx| this.leave(cx)));
        if self.entries.is_empty() {
            return root.child(if self.loading {
                self.loading_state(cx).into_any_element()
            } else {
                self.empty_state(cx)
            });
        }
        // A drag of the picture follows the pointer anywhere in the window
        // until the button is released, as pointer capture does.
        if self.compare.drag.is_some() {
            let entity = cx.weak_entity();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ =
                                    moved.update(cx, |this, cx| this.dragged(event.position, cx));
                            }
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = released.update(cx, |this, cx| {
                                    this.compare.drag = None;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        root = match self.mode.clone() {
            Mode::List => {
                let list = self.list(window, cx);
                let footer = self.footer(window, cx);
                root.child(list).child(footer)
            }
            Mode::Compare { set, shown } => root.child(self.compare_view(set, shown, window, cx)),
            Mode::Verify { sets, current } => {
                root.child(self.verify_view(sets, current, window, cx))
            }
        };
        if let Some(shown) = &self.preview.shown {
            let (overlay, fading) = crate::grid::preview::overlay(shown, window);
            if fading {
                window.request_animation_frame();
            }
            root = root.children(overlay);
        }
        let menu = self.menu.layer(window, cx);
        root.children(menu)
    }
}

impl Render for DuplicatesToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bar = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(8.));
        let Some(screen) = self.screen.upgrade() else {
            return bar;
        };
        let this = screen.read(cx);
        if this.entries.is_empty() {
            return bar;
        }
        let weak = self.screen.clone();
        let act = move |action: fn(&mut DuplicatesScreen, &mut Context<DuplicatesScreen>)| {
            let screen = weak.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = screen.update(cx, action);
            }
        };
        // The controls sit at the row's right end, as every screen's do,
        // clear of the scope title on the left half; the least needed give
        // way first when the row is short.
        let width = f32::from(this.bounds.size.width);
        let roomy = |needed: f32| width == 0. || width >= needed;
        let caption = |text: String| {
            div()
                .flex_none()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .whitespace_nowrap()
                .child(text)
        };
        match this.mode.clone() {
            Mode::List => {
                let (identical, similar) = this.tab_counts();
                let tab = this.tab;
                let sort = this.sort;
                let tabs_screen = self.screen.clone();
                let sort_screen = self.screen.clone();
                let tabs = ui::segmented::segmented(
                    "duplicates-tabs",
                    ui::segmented::Look::Tabs,
                    vec![
                        ui::segmented::tab(format!("{} {identical}", en("Identical"))),
                        ui::segmented::tab(format!("{} {similar}", en("Similar"))),
                    ],
                    Some(if tab == Tab::Identical { 0 } else { 1 }),
                )
                .name(t("Duplicates"))
                .on_select(move |index, _, cx| {
                    let _ = tabs_screen.update(cx, |this, cx| {
                        this.set_tab(
                            if index == 0 {
                                Tab::Identical
                            } else {
                                Tab::Similar
                            },
                            cx,
                        )
                    });
                });
                let value = Sort::VALUES
                    .iter()
                    .find(|(each, _)| *each == sort)
                    .map_or("saving", |(_, value)| value);
                let sort = ui::select(
                    "duplicates-sort",
                    value,
                    vec![
                        ui::choice("saving", en("Largest saving")),
                        ui::choice("files", en("Most files")),
                        ui::choice("alike", en("Most alike")),
                    ],
                )
                .name(en("Sort sets"))
                .on_change(move |value, _, cx| {
                    let sort = Sort::VALUES
                        .iter()
                        .find(|(_, each)| *each == value)
                        .map_or(Sort::Saving, |(sort, _)| *sort);
                    let _ = sort_screen.update(cx, |this, cx| this.set_sort(sort, cx));
                });
                // The tabs take their presses, which would otherwise start
                // a move of the window from the titlebar.
                bar.child(div().flex_1())
                    .child(div().flex_none().occlude().child(tabs))
                    .children(roomy(760.).then(|| div().flex_none().child(sort)))
                    .child(
                        ui::icon_button("duplicates-scan", "action-refresh")
                            .disabled(this.scanning || this.busy)
                            .tip(ui::tip(t("Scan library")))
                            .on_click(act(|this, cx| this.scan(cx))),
                    )
            }
            Mode::Compare { set, shown } => {
                let Some(index) = this.entry_index(set) else {
                    return bar;
                };
                let entry = &this.entries[index];
                let position = this
                    .order
                    .iter()
                    .position(|&each| each == index)
                    .map_or(0, |at| at + 1);
                let keeper = this.keeper(index);
                let counter = format!(
                    "{} {position} {} {} · {} {} {} {}",
                    en("Set"),
                    en("of"),
                    this.order.len(),
                    en("file"),
                    shown + 1,
                    en("of"),
                    entry.set.files.len()
                );
                let zoom = this.zoom_mode();
                let hover = self.screen.clone();
                bar.child(div().flex_1())
                    .children(roomy(560.).then(|| caption(counter)))
                    .child(
                        ui::icon_button("duplicates-difference", "toolbar-difference")
                            .disabled(shown == keeper)
                            .tip(ui::tip(t("Show differences while held")))
                            .on_hover(move |hovered, _, cx| {
                                let _ = hover.update(cx, |this, cx| {
                                    if this.compare.difference_hovered != hovered {
                                        this.compare.difference_hovered = hovered;
                                        cx.notify();
                                    }
                                });
                            }),
                    )
                    .child(
                        ui::icon_button("duplicates-fit", "toolbar-fit")
                            .toggled(zoom == super::compare::Mode::Fit)
                            .tip(ui::tip(t("Zoom to fit")).shortcut("dup.fitToWindow", cx))
                            .on_click(act(DuplicatesScreen::fit)),
                    )
                    .child(
                        ui::icon_button("duplicates-actual", "toolbar-actual-size")
                            .toggled(zoom == super::compare::Mode::Actual)
                            .tip(ui::tip(t("Actual pixels")).shortcut("view.actualSize", cx))
                            .on_click(act(DuplicatesScreen::actual)),
                    )
            }
            Mode::Verify { sets, .. } => bar.child(div().flex_1()).child(caption(format!(
                "{} {}",
                sets.len(),
                en("sets differ")
            ))),
        }
    }
}
