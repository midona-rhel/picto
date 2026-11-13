//! The Duplicates screen's drawing: the loading and empty states, the two
//! candidate cards with their previews and facts, the footer of decisions,
//! and the titlebar controls.
use super::{
    ActualPixels, CARD_HEADER, CARD_METADATA, COMPACT_WIDTH, CONTEXT, Decision, Drag,
    DuplicatesScreen, DuplicatesToolbar, FitPair, KeepBoth, KeepLeft, KeepRight, NARROW_WIDTH,
    NextPair, NotDuplicate, PREVIEW_MIN, PreviousPair, Side, SmartMerge, compare,
};
use crate::theme::Text as _;
use crate::ui::action_button::Look;
use crate::{motion, theme, ui};
use gpui::*;
use picto_library::{Rating, RootKind};
use std::sync::Arc;

/// The rating as the Electron screen prints it: the stored name.

fn rating_name(rating: Rating) -> &'static str {
    match rating {
        Rating::Unrated => "unrated",
        Rating::One => "one",
        Rating::Two => "two",
        Rating::Three => "three",
        Rating::Four => "four",
        Rating::Five => "five",
    }
}

impl DuplicatesScreen {
    /// `.centerState`: the queue's first read, with its message after 200 ms.
    fn loading_state(&self, cx: &App) -> Div {
        let theme = theme::theme(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .p(px(24.))
            .text_color(theme.text_secondary())
            .children(
                self.loading_message
                    .then_some("Loading duplicate review queue..."),
            )
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let resolved = self.initial_total.saturating_sub(self.total);
        let (title, description) = match &self.error {
            Some(error) => ("Unable to load duplicate review".to_owned(), error.clone()),
            None if resolved > 0 => (
                "Review complete".to_owned(),
                format!("{resolved} decisions saved."),
            ),
            None => (
                "No duplicate pairs".to_owned(),
                "Scan the library to find similar images.".to_owned(),
            ),
        };
        let action = ui::empty_state_action(
            "duplicates-scan",
            "action-refresh",
            if self.error.is_some() {
                "Retry"
            } else {
                "Scan library"
            },
        )
        .disabled(self.scanning || self.resolving)
        .on_click(cx.listener(|this, _, _, cx| this.scan(cx)));
        ui::empty_state(
            "empty-copy",
            title,
            description,
            vec![action.into_any_element()],
            cx,
        )
        // `.progress`: the 180 × 2 slot of the scan's progress bar, which
        // keeps its place while empty.
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

    /// One candidate (`MediaCard`): its header with Keep, its preview, and
    /// its facts.
    fn card(
        &self,
        index: usize,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let side = &shown.sides[index];
        let (label, keep) = if index == 0 {
            ("Left candidate", "Keep left")
        } else {
            ("Right candidate", "Keep right")
        };
        // Keeps this file and merges the other's item into it; the other
        // file is deleted from the copy.
        let mut keep_button = ui::action_button(("duplicate-keep", index), Look::Standard)
            .icon("check", 15.)
            .height(theme::CONTROL_MD)
            .text_size(theme::TEXT_SM)
            .tip(ui::tip(keep).shortcut(
                if index == 0 {
                    "dup.keepLeft"
                } else {
                    "dup.keepRight"
                },
                cx,
            ))
            .disabled(self.resolving)
            .on_click(cx.listener(move |this, _, _, cx| this.keep(index, cx)));
        keep_button = if compact {
            keep_button.width(theme::CONTROL_MD).padding(0.)
        } else {
            keep_button.padding(10.).label(keep)
        };
        let header = div()
            .h(px(CARD_HEADER))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(8.))
            .child(
                div()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_SM)
                    .child(label),
            )
            .child(keep_button);
        // `.mediaCard::after`: the green ring around the file Smart merge
        // would keep, 5px outside the card, fading over 140 ms.
        let ring = self.survivor.value();
        if self.survivor.animating() {
            window.request_animation_frame();
        }
        let survivor = (ring > 0. && self.survivor_side() == Some(index)).then(|| {
            let at = |color: u32| {
                let color = rgba(color);
                Rgba {
                    a: color.a * ring,
                    ..color
                }
            };
            div()
                .absolute()
                .top(px(-5.))
                .left(px(-5.))
                .right(px(-5.))
                .bottom(px(-5.))
                .rounded(px(theme::RADIUS_MD + 5.))
                .bg(at(theme::MERGE_SURVIVOR_FILL))
                .border_1()
                .border_color(at(theme::MERGE_SURVIVOR_RING))
        });
        div()
            .relative()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(theme::RADIUS_MD))
            .child(header)
            .child(self.preview(index, window, cx))
            .child(self.metadata(side, cx))
            .children(survivor)
    }

    /// The preview pane: the image in its frame, zoomed and panned with the
    /// other, or the difference picture while Show Difference is held.
    fn preview(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let side = &shown.sides[index];
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    std::mem::replace(&mut this.panes[index], bounds) != bounds
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        // The frame: where the image sits in the pane. Until the pane has
        // been measured, the image fills it, fitted, which is the same.
        let (pane, image) = (self.pane(index), side.image());
        let frame = |layer: Div| {
            if pane.width > 0. && pane.height > 0. && image.width > 0. && image.height > 0. {
                let frame = self.zoom.frame(pane, image);
                layer
                    .absolute()
                    .left(px(frame.left))
                    .top(px(frame.top))
                    .w(px(frame.width))
                    .h(px(frame.height))
            } else {
                layer.absolute().inset_0()
            }
        };
        let fill = |image: Arc<RenderImage>, fit: ObjectFit| {
            img(image).absolute().inset_0().size_full().object_fit(fit)
        };
        let mut inside = div()
            .absolute()
            .inset(px(1.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .child(measure)
            .children(side.details.is_none().then(|| {
                div()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_SM)
                    .child("Media details unavailable")
            }));
        if self.difference_hovered && !self.resolving {
            // `.differenceComposite`: on black.
            inside = inside.child(
                frame(div().bg(rgb(theme::DIFFERENCE_BACKGROUND))).children(
                    shown
                        .difference
                        .clone()
                        .map(|image| fill(image, ObjectFit::Fill)),
                ),
            );
        } else {
            let opacity = side.full_opacity();
            // Painting continues only while an arrived original fades in.
            if side.full_since.is_some() && opacity < 1. {
                window.request_animation_frame();
            }
            let mut layers = frame(div());
            if opacity < 1.
                && let Some(thumbnail) = side.thumbnail.clone()
            {
                layers = layers.child(fill(thumbnail, ObjectFit::Contain));
            }
            if let Some(full) = side.full.clone() {
                layers = layers.child(fill(full, ObjectFit::Contain).opacity(opacity));
            }
            inside = inside.child(layers);
        }
        let dragging = self.drag.as_ref().is_some_and(|drag| drag.side == index);
        div()
            .id(("duplicate-preview", index))
            .relative()
            .flex_1()
            .min_h(px(PREVIEW_MIN))
            .rounded(px(theme::RADIUS_MD))
            .bg(rgba(theme::BLACK_30))
            .cursor(if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .child(inside)
            // The border over the image, which may cover the pane's edge.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border_secondary()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    if event.click_count == 2 {
                        this.drag = None;
                        this.fit(cx);
                    } else {
                        this.drag = Some(Drag {
                            side: index,
                            from: event.position,
                            center: this.zoom.center(),
                        });
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                this.wheel(index, event, window, cx);
                cx.stop_propagation();
            }))
    }

    /// The file's facts (`.metadata`).
    fn metadata(&self, side: &Side, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let name = side.media.as_ref().map_or_else(
            || side.file.file_hash.clone(),
            |media| media.media_name.clone(),
        );
        let details = side.details.as_ref();
        let row = |label: &'static str, value: String| {
            ui::property_row(label, value).mono(theme::FONT_MONO)
        };
        let mut rows = vec![
            row(
                "Resolution",
                compare::dimensions(side.file.pixel_width, side.file.pixel_height),
            ),
            row("Size", crate::text::size_to_megabytes(side.file.size_bytes)),
            row("Format", side.file.mime_type.clone()),
            row(
                "Rating",
                details
                    .map_or("Unrated", |details| rating_name(details.rating))
                    .into(),
            ),
            row(
                "Tags",
                details
                    .map_or(0, |details| details.tag_ids.len())
                    .to_string(),
            ),
            row(
                "Created",
                details
                    .and_then(|details| details.root.captured_at_ms)
                    .filter(|at| *at != 0)
                    .map_or_else(|| "Unknown".into(), compare::short_date),
            ),
            row(
                "Added",
                details.map_or_else(
                    || "Unknown".into(),
                    |details| compare::short_date(details.root.imported_at_ms),
                ),
            ),
        ];
        if let Some(root) = details
            .map(|details| &details.root)
            .filter(|root| root.kind == RootKind::Collection)
        {
            let group = if root.name.is_empty() {
                format!("Group {}", root.root_id.0)
            } else {
                root.name.clone()
            };
            rows.push(row("Group", group));
        }
        rows.push(row("Occurrences", side.occurrences.to_string()));
        div()
            .h(px(CARD_METADATA))
            .flex_none()
            .pt(px(10.))
            .px(px(4.))
            .pb(px(12.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(4.))
                    .text_color(theme.text())
                    .text_size(px(theme::TEXT_MD))
                    .line_height(px(16.))
                    .truncate()
                    .child(name),
            )
            .children(rows)
    }

    /// The decisions (`.footer`).
    fn footer(
        &self,
        compact: bool,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let shown = self.shown.as_ref().expect("a pair is on screen");
        let button = |id: &'static str, look: Look| {
            ui::action_button(id, look)
                .height(36.)
                .disabled(self.resolving)
        };
        let not_duplicates = button("duplicates-not", Look::Standard)
            .icon("action-x", 15.)
            .label("Not duplicates")
            .tip(ui::tip("These are different media").shortcut("dup.notDuplicate", cx))
            .on_click(cx.listener(|this, _, _, cx| this.keep_both(cx)));
        let keep_both = button("duplicates-keep-both", Look::Standard)
            .icon("action-copy", 15.)
            .label("Keep both")
            .tip(ui::tip("Keep both files").shortcut("dup.keepBoth", cx))
            .on_click(cx.listener(|this, _, _, cx| this.keep_both(cx)));
        // Keeps the better file of the two and merges the other into it,
        // ringing the survivor in green while hovered.
        let hover = cx.weak_entity();
        let smart_merge = button("duplicates-smart-merge", Look::Primary)
            .icon("action-arrows-join", 16.)
            .label("Smart merge")
            .tip(
                ui::tip("Keep the stronger file and preserve item metadata")
                    .shortcut("dup.smartMerge", cx),
            )
            .on_click(cx.listener(|this, _, _, cx| this.decide(Decision::Smart, cx)))
            .on_hover(move |hovered, _, cx| {
                let _ = hover.update(cx, |this, cx| this.set_smart_hovered(hovered, cx));
            });
        let entity = cx.weak_entity();
        let difference = button("duplicates-difference", Look::Standard)
            .icon("toolbar-difference", 16.)
            .label("Show Difference")
            // `.differenceButton`'s padding, gap and type lose to the shared
            // `.btn` in the built stylesheet, so it looks like its neighbours.
            .tip(ui::tip("Show differences while held"))
            .on_hover(move |hovered, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.difference_hovered != hovered {
                        this.difference_hovered = hovered;
                        cx.notify();
                    }
                });
            });
        let similarity = ui::tooltip::attach(
            div()
                .id("duplicates-similarity")
                .ml(px(8.))
                .text_color(theme.text_secondary())
                .text(theme::TEXT_SM)
                .whitespace_nowrap()
                .child(compare::similarity_label(shown.distance)),
            ("duplicates-similarity", 1usize),
            ui::tip("Perceptual similarity is not pixel equality"),
            theme::TEXT_SM + 2.,
            window,
            cx,
        );
        let mut actions = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(not_duplicates)
            .child(keep_both)
            .child(smart_merge)
            .child(difference)
            .child(similarity);
        if compact {
            actions = actions.flex_wrap();
        }
        div()
            .min_h(px(58.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .py(px(8.))
            .px(px(if narrow { 8. } else { 18. }))
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.background())
            .child(actions)
    }

    /// The two cards: side by side, or stacked in a scrolling column when
    /// the screen is narrow.
    fn comparison(
        &self,
        compact: bool,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    let height = f32::from(bounds.size.height);
                    std::mem::replace(&mut this.comparison_height, height) != height
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let (left, right) = (
            self.card(0, compact, window, cx),
            self.card(1, compact, window, cx),
        );
        if narrow {
            // One column, each card at least as tall as its rows allow and
            // sharing any height left over (`grid-template-columns: 1fr`).
            let gap = 10.;
            let card = ((self.comparison_height - 2. * 10. - gap) / 2.)
                .max(CARD_HEADER + PREVIEW_MIN + CARD_METADATA);
            return div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(measure)
                .child(
                    div()
                        .id("duplicates-comparison")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(gap))
                                .p(px(10.))
                                // `scrollbar-gutter: stable`
                                .pr(px(10. + ui::scrollbar::GUTTER))
                                .child(left.h(px(card)))
                                .child(right.h(px(card))),
                        ),
                )
                .child(ui::scrollbar(
                    "duplicates-scrollbar",
                    &self.scroll,
                    window,
                    cx,
                ))
                .into_any_element();
        }
        let (gap, x, y) = if compact {
            (8., 10., 10.)
        } else {
            (12., 18., 14.)
        };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(gap))
            .px(px(x))
            .py(px(y))
            .child(measure)
            .child(left.flex_1())
            .child(right.flex_1())
            .into_any_element()
    }
}

impl Render for DuplicatesScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let entity = cx.weak_entity();
        let measure = canvas(
            move |bounds, window, cx| {
                Self::measured(&entity, window, cx, |this| {
                    let width = f32::from(bounds.size.width);
                    std::mem::replace(&mut this.width, width) != width
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
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
            .child(measure)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousPair, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NextPair, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &FitPair, _, cx| this.fit(cx)))
            .on_action(cx.listener(|this, _: &ActualPixels, _, cx| this.actual(cx)))
            .on_action(cx.listener(|this, _: &KeepBoth, _, cx| this.keep_both(cx)))
            .on_action(cx.listener(|this, _: &NotDuplicate, _, cx| this.keep_both(cx)))
            .on_action(cx.listener(|this, _: &KeepLeft, _, cx| this.keep(0, cx)))
            .on_action(cx.listener(|this, _: &KeepRight, _, cx| this.keep(1, cx)))
            .on_action(cx.listener(|this, _: &SmartMerge, _, cx| this.decide(Decision::Smart, cx)));
        if self.shown.is_none() {
            return root.child(if self.loading {
                self.loading_state(cx).into_any_element()
            } else {
                self.empty_state(cx)
            });
        }
        // A drag follows the pointer anywhere in the window until the
        // button is released, as pointer capture does.
        if self.drag.is_some() {
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
                                    this.drag = None;
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
        // `mergePreviewActive`: hovered, nothing resolving, a clear winner.
        let ring = self.smart_hovered && !self.resolving && self.survivor_side().is_some();
        self.survivor.set(ring, motion::SURVIVOR_RING);
        let compact = self.width > 0. && self.width <= COMPACT_WIDTH;
        let narrow = self.width > 0. && self.width <= NARROW_WIDTH;
        let comparison = self.comparison(compact, narrow, window, cx);
        let footer = self.footer(compact, narrow, window, cx);
        root.children(self.scan_progress.then(|| ui::progress_bar().height(2.)))
            .child(comparison)
            .child(footer)
    }
}

impl Render for DuplicatesToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let bar = div().flex_1().min_w_0().h_full().flex().items_center();
        let Some(model) = self
            .screen
            .upgrade()
            .and_then(|screen| screen.read(cx).toolbar_model())
        else {
            return bar;
        };
        // The buttons call the screen directly: the screen repaints these
        // controls itself, which it could not do while they were updating.
        let act = |action: fn(&mut DuplicatesScreen, &mut Context<DuplicatesScreen>)| {
            let screen = self.screen.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = screen.update(cx, action);
            }
        };
        bar.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .ml(px(4.))
                        .text_color(theme.text_secondary())
                        .text(theme::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .whitespace_nowrap()
                        .child(format!("{} / {}", model.index + 1, model.total)),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    ui::icon_button("duplicates-fit", "toolbar-fit")
                        .toggled(model.fit)
                        .tip(ui::tip("Zoom to fit").shortcut("view.fitWindow", cx))
                        .on_click(act(DuplicatesScreen::fit)),
                )
                .child(
                    ui::icon_button("duplicates-actual", "toolbar-actual-size")
                        .toggled(model.actual)
                        .tip(ui::tip("Actual pixels").shortcut("view.actualSize", cx))
                        .on_click(act(DuplicatesScreen::actual)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            ui::icon_button("duplicates-previous", "toolbar-chevron-left")
                                .disabled(!model.can_previous || model.disabled)
                                .tip(ui::tip("Previous pair").shortcut("dup.prevPair", cx))
                                .on_click(act(|screen, cx| screen.step(-1, cx))),
                        )
                        .child(
                            ui::icon_button("duplicates-next", "toolbar-chevron-right")
                                .disabled(!model.can_next || model.disabled)
                                .tip(ui::tip("Next pair").shortcut("dup.nextPair", cx))
                                .on_click(act(|screen, cx| screen.step(1, cx))),
                        ),
                ),
        )
    }
}
