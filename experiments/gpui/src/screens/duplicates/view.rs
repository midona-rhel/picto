//! The Duplicates screen's drawing: its loading, empty and error states,
//! the queue beside the stage, and the titlebar controls.
use super::{
    ActualPixels, CONTEXT, DuplicatesScreen, DuplicatesToolbar, FitPair, Merge, NextFile, NextSet,
    NotDuplicate, PreviousFile, PreviousSet, Skip, SmartMerge,
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
        let root = div()
            .id("duplicates")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .overflow_hidden()
            .flex()
            .bg(theme.background())
            .text_color(theme.text())
            .font_family(theme::FONT_UI)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousFile, _, cx| this.flip(-1, cx)))
            .on_action(cx.listener(|this, _: &NextFile, _, cx| this.flip(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousSet, _, cx| this.step_set(-1, cx)))
            .on_action(cx.listener(|this, _: &NextSet, _, cx| this.step_set(1, cx)))
            .on_action(cx.listener(|this, _: &Merge, _, cx| this.merge(cx)))
            .on_action(cx.listener(|this, _: &SmartMerge, _, cx| this.smart_merge(cx)))
            .on_action(cx.listener(|this, _: &Skip, _, cx| this.skip(cx)))
            .on_action(cx.listener(|this, _: &NotDuplicate, _, cx| this.not_duplicates(cx)))
            .on_action(cx.listener(|this, _: &FitPair, _, cx| this.fit(cx)))
            .on_action(cx.listener(|this, _: &ActualPixels, _, cx| this.actual(cx)));
        if self.entries.is_empty() {
            return root.child(if self.loading {
                self.loading_state(cx).into_any_element()
            } else {
                self.empty_state(cx)
            });
        }
        // A drag of the picture follows the pointer anywhere in the window
        // until the button is released, as pointer capture does.
        let drag = self.stage.drag.is_some().then(|| {
            let entity = cx.weak_entity();
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = moved.update(cx, |this, cx| this.dragged(event.position, cx));
                        }
                    });
                    let released = entity.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            let _ = released.update(cx, |this, cx| {
                                this.stage.drag = None;
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        });
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let width = f32::from(bounds.size.width);
                let _ = entity.update(cx, |this, cx| {
                    if this.width != width {
                        this.width = width;
                        this.changed(cx);
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let queue = self.queue(window, cx);
        let stage = self.stage(window, cx);
        let menu = self.menu.layer(window, cx);
        root.child(measure)
            .children(drag)
            .child(queue)
            .child(stage)
            .children(self.scan_progress.then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .child(ui::progress_bar().height(2.))
            }))
            .children(menu)
    }
}

impl Render for DuplicatesToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let bar = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(4.))
            .relative()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let width = f32::from(bounds.size.width);
                        let _ = entity.update(cx, |this, cx| {
                            if this.width != width {
                                this.width = width;
                                cx.notify();
                                window.request_animation_frame();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
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
        let zoom = this.zoom_mode();
        // The controls sit at the row's right end, as every screen's do, in
        // the half the scope title leaves them; the scan gives way first.
        let roomy = self.width == 0. || self.width / 2. >= 120.;
        let side_by_side = this.side_by_side();
        let toggle_side = {
            let screen = self.screen.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = screen.update(cx, |this, cx| this.set_side_by_side(!side_by_side, cx));
            }
        };
        bar.child(div().flex_1())
            .child(
                ui::icon_button("duplicates-side-by-side", "toolbar-columns")
                    .toggled(side_by_side)
                    .tip(ui::tip(en("Side by Side")))
                    .on_click(toggle_side),
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
            .children(roomy.then(|| {
                ui::icon_button("duplicates-scan", "action-refresh")
                    .disabled(this.scanning || this.busy)
                    .tip(ui::tip(t("Scan library")))
                    .on_click(act(|this, cx| this.scan(cx)))
            }))
    }
}
