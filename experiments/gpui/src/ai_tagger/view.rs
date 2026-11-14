//! The review panel's drawing (`AiTaggerPanel.module.css`): the rail of
//! views and models with Run, the item reviewed with its preview and
//! thumbnails, the list of suggested tags, and the footer's cutoff and
//! Apply.
use super::panel::{AiTagger, HEIGHT, ViewMode};
use super::review::{self, Empty, EmptyState, ReviewTag};
use crate::grid::format;
use crate::i18n::t;
use crate::tags::Preferences;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look,
    check::{self, Mark},
    overlay_shell,
};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

/// `.sidebar`, `.reviewPane`, `.content`.
const RAIL: f32 = 190.;
const REVIEW: f32 = 220.;
const LIST: f32 = 370.;
/// `.previewFrame`'s largest side.
const PREVIEW: f32 = 200.;

impl AiTagger {
    /// `.sidebarItem`: a 24px rail row with a mark, a name and a badge.
    fn rail_item(
        &self,
        id: impl Into<ElementId>,
        mark: AnyElement,
        label: SharedString,
        badge: SharedString,
        active: bool,
        disabled: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let mut row = div()
            .id(id)
            .w_full()
            .h(px(24.))
            .flex_none()
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .whitespace_nowrap()
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(122.))
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .child(badge),
            );
        if active {
            row = row.bg(theme.surface_active());
        }
        if disabled {
            row = row.opacity(0.45);
        } else {
            row = row
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_hover()));
        }
        row
    }

    fn dot(color: impl Into<Hsla>) -> AnyElement {
        div()
            .size(px(8.))
            .flex_none()
            .rounded_full()
            .bg(color.into())
            .opacity(0.9)
            .into_any_element()
    }

    fn rail(
        &self,
        suggested: usize,
        below: usize,
        checked: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let mut rail = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(RAIL))
            .overflow_hidden()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .border_r_1()
            .border_color(theme.border_secondary())
            .opacity(self.rail_shown.value_with(motion::EASE))
            .child(
                self.rail_item(
                    "ai-suggested",
                    Self::dot(theme.primary()),
                    t("Suggested").into(),
                    suggested.to_string().into(),
                    self.view == ViewMode::Suggested,
                    false,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = ViewMode::Suggested;
                    cx.notify();
                })),
            )
            .child(
                self.rail_item(
                    "ai-below",
                    Self::dot(theme.contrast(25.)),
                    t("Below cutoff").into(),
                    below.to_string().into(),
                    self.view == ViewMode::Below,
                    false,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = ViewMode::Below;
                    cx.notify();
                })),
            );
        if !self.models.is_empty() {
            rail = rail.child(
                div()
                    .h(px(1.))
                    .flex_none()
                    .my(px(4.))
                    .mx(px(10.))
                    .bg(theme.border_secondary()),
            );
        }
        for (index, model) in self.models.iter().enumerate() {
            let chosen = self.run_models.contains(&model.slug);
            let count = review::model_count(&self.predictions, &model.slug);
            let badge = if model.downloaded && count > 0 {
                count.to_string()
            } else {
                "·".to_owned()
            };
            let mark = check::check_box(
                if chosen { Mark::Checked } else { Mark::Empty },
                check::Size::Compact,
            )
            .into_any_element();
            let tip = if model.downloaded {
                model.dataset.clone()
            } else {
                crate::tf!(
                    "{value0} is not downloaded — get it in Settings",
                    model.label
                )
            };
            let enabled = model.downloaded && !self.running;
            let slug = model.slug.clone();
            let mut row = self.rail_item(
                ("ai-model", index),
                mark,
                model.label.clone().into(),
                badge.into(),
                chosen,
                !model.downloaded,
                cx,
            );
            if enabled {
                row = row.on_click(cx.listener(move |this, _, _, cx| this.toggle_model(&slug, cx)));
            }
            rail = rail.child(ui::tooltip::attach(
                row,
                ("ai-model-tip", index),
                ui::tip(tip),
                24.,
                window,
                cx,
            ));
        }
        let run = ui::action_button(
            "ai-run",
            if checked == 0 {
                Look::Primary
            } else {
                Look::Standard
            },
        )
        .label(if self.running {
            t("Running…")
        } else {
            t("Run")
        })
        .full_width()
        .height(theme::CONTROL_SM)
        .text_size(theme::TEXT_SM)
        .disabled(self.running || self.run_models.is_empty() || self.items.is_empty())
        .on_click(cx.listener(|this, _, _, cx| this.run(cx)));
        rail.child(
            div()
                .mt_auto()
                .pt(px(8.))
                .border_t_1()
                .border_color(theme.border_secondary())
                .child(run),
        )
    }

    /// `.navButton`: a 24px chevron that dims when there is nowhere to go.
    fn nav_button(
        &self,
        id: &'static str,
        icon: &'static str,
        tip: &'static str,
        shortcut: &'static str,
        delta: isize,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let mut button = div()
            .id(id)
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .child(ui::icon(icon, 15., theme.text()));
        if enabled {
            button = button
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_hover()))
                .on_click(cx.listener(move |this, _, _, cx| this.move_active(delta, cx)));
        } else {
            button = button.opacity(0.28);
        }
        ui::tooltip::attach(
            button,
            SharedString::from(format!("{id}-tip")),
            ui::tip(tip).shortcut(shortcut, cx),
            24.,
            window,
            cx,
        )
    }

    /// The item reviewed (`.reviewPane`).
    fn review_pane(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let index = self.active_index();
        let count = self.items.len();
        let number = if count > 0 { index + 1 } else { 0 };
        let root = self.active.and_then(|id| self.root(id)).cloned();
        let prediction = self
            .active
            .and_then(|id| self.predictions.iter().find(|p| p.root_id == id));
        let this_running = self.running && self.progress.current == self.active;

        let navigation = div()
            .h(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .child(self.nav_button(
                "ai-previous",
                "ai-chevron-left",
                t("Previous item"),
                "view.prevImage",
                -1,
                index > 0,
                window,
                cx,
            ))
            .child(
                div()
                    .min_w(px(48.))
                    .flex()
                    .justify_center()
                    .font_family(theme::FONT_MONO)
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .child(format!("{number} / {count}")),
            )
            .child(self.nav_button(
                "ai-next",
                "ai-chevron-right",
                t("Next item"),
                "view.nextImage",
                1,
                index + 1 < count,
                window,
                cx,
            ));

        // `.preview` and `.previewFrame`: the cover fitted in 200px, on its
        // first palette colour, with the glass edge over it.
        let mut frame = div()
            .relative()
            .rounded(px(theme::RADIUS_MD))
            .overflow_hidden();
        match &root {
            Some(root) => {
                let path = self.session.read(cx).thumbnail(&root.preview_hash);
                let resource = Resource::Path(path.clone().into());
                let natural = self.thumbnails.read(cx).size(&resource).or(root
                    .preview_size
                    .map(|(width, height)| (width as f32, height as f32)));
                let (width, height) = match natural {
                    Some((width, height)) if width > 0. && height > 0. => {
                        let scale = (PREVIEW / width).min(PREVIEW / height).min(1.);
                        (width * scale, height * scale)
                    }
                    _ => (PREVIEW, PREVIEW),
                };
                if let Some(color) = &root.preview_color {
                    frame = frame.bg(rgb(format::lab_to_rgb(color.l, color.a, color.b)));
                }
                frame = frame.w(px(width.round())).h(px(height.round())).child(
                    img(path)
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            }
            None => {
                frame = frame.child(
                    div()
                        .p(px(16.))
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .text_center()
                        .child(t("Preparing preview…")),
                );
            }
        }
        frame = frame.child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(rgba(theme::BLACK_10)),
        );
        if this_running {
            frame = frame.child(
                div()
                    .absolute()
                    .left(px(8.))
                    .right(px(8.))
                    .bottom(px(8.))
                    .px(px(7.))
                    .py(px(5.))
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.border())
                    .bg(theme.glass())
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text())
                    .text_center()
                    .child(t("Analyzing this item…")),
            );
        }
        let frame_shadow = canvas(
            |_, _, _| {},
            |bounds, _, window, _| {
                window.paint_drop_shadows_outside(
                    bounds,
                    Corners::all(px(theme::RADIUS_MD)),
                    &[BoxShadow::new(px(0.), px(0.), rgba(theme::BLACK_10).into())
                        .blur_radius(px(5.))],
                );
            },
        )
        .absolute()
        .inset_0();
        let preview = div()
            .w_full()
            .min_h(px(PREVIEW))
            .mb(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .child(div().relative().child(frame_shadow).child(frame));

        let name = root.as_ref().map_or_else(
            // i18n: production writes it in English
            || format!("Item {}", number.max(1)),
            |root| root.label.clone(),
        );
        let meta = match prediction {
            Some(prediction) => {
                crate::tf!("{value0} tag predictions", prediction.predictions.len())
            }
            None if self.running => t("Waiting for analysis").to_owned(),
            None => t("No prediction result").to_owned(),
        };

        let mut thumbnails = div()
            .id("ai-thumbnails")
            .min_h(px(48.))
            .mt_auto()
            .p(px(3.))
            .flex()
            .gap(px(5.))
            .overflow_x_scroll()
            .track_scroll(&self.rail_scroll);
        for (place, id) in self.rail.iter().enumerate() {
            let Some(root) = self.root(*id) else {
                continue;
            };
            let active = self.active == Some(*id);
            let path = self.session.read(cx).thumbnail(&root.preview_hash);
            let id = *id;
            let button = div()
                .id(("ai-thumbnail", place))
                .size(px(42.))
                .flex_none()
                .rounded(px(6.))
                .overflow_hidden()
                .bg(theme.control_surface())
                .border_1()
                .border_color(if active {
                    theme.text()
                } else {
                    theme.border_secondary()
                })
                .opacity(if active { 1. } else { 0.62 })
                .cursor_pointer()
                .when(!active, |button| button.hover(|style| style.opacity(0.86)))
                .child(
                    img(path)
                        .image_cache(&self.thumbnails)
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.show(id, cx)));
            let number = self
                .items
                .iter()
                .position(|item| *item == id)
                .unwrap_or(place);
            thumbnails = thumbnails.child(ui::tooltip::attach(
                button,
                ("ai-thumbnail-tip", place),
                ui::tip(crate::tf!("Review item {value0}", number + 1)),
                42.,
                window,
                cx,
            ));
        }

        div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(LIST))
            .w(px(REVIEW))
            .min_w_0()
            .p(px(9.))
            .flex()
            .flex_col()
            .gap(px(7.))
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(navigation)
            .child(preview)
            // Mantine's body `line-height: 1.55`.
            .child(
                div()
                    .text(theme::TEXT_MD)
                    .line_height(px(theme::TEXT_MD * 1.55))
                    .text_color(theme.text())
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .text(theme::TEXT_CAPTION)
                    .line_height(px(theme::TEXT_CAPTION * 1.55))
                    .text_color(theme.text_tertiary())
                    .child(meta),
            )
            .child(thumbnails)
    }

    /// `.tagRow`: the check, the bookmark in the group's colour, the name,
    /// the models that suggested it and its confidence.
    fn tag_row(
        &self,
        index: usize,
        item: picto_library::RootId,
        tag: &ReviewTag,
        preferences: &Preferences,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let checked = self.overrides.is_checked(item, tag, self.cutoff());
        let color = rgb(crate::tags::group_color(&tag.namespace, preferences));
        let mut models = tag.models.clone();
        models.sort_by(|a, b| b.1.total_cmp(&a.1));
        let evidence = models
            .iter()
            .map(|(slug, _)| {
                self.models
                    .iter()
                    .find(|model| model.slug == *slug)
                    .map_or(slug.clone(), |model| model.label.clone())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut name = div()
            .flex()
            .max_w(relative(0.44))
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_color(theme::blend(
                theme.text(),
                color,
                if theme.is_light() { 0.2 } else { 0.12 },
            ));
        if tag.shows_namespace() {
            name = name.child(div().flex_none().child(format!("{}:", tag.namespace)));
        }
        // `.tagName` is a flex box, so its overflow is clipped without an
        // ellipsis.
        name = name.child(div().flex_none().child(tag.subtag.clone()));
        let row_tag = tag.clone();
        let mut row = div()
            .id(("ai-tag", index))
            .relative()
            .h(px(26.))
            .mx(px(4.))
            .px(px(6.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .child(check::check_box(
                if checked { Mark::Checked } else { Mark::Empty },
                check::Size::Compact,
            ))
            .child(ui::icon("ai-bookmark", 13., color).flex_none())
            .child(name)
            .child(
                div()
                    .flex_none()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child("·"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .child(evidence),
            )
            .child(
                div()
                    .min_w(px(32.))
                    .ml_auto()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .font_family(theme::FONT_MONO)
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .child(format!("{}%", (tag.confidence * 100.).round() as i64)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_tag(item, &row_tag, cx)));
        if checked {
            row = row.bg(theme.surface_active()).child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.contrast(25.)),
            );
        } else {
            row = row.hover(move |style| style.bg(theme.surface_hover()));
        }
        row
    }

    fn list(
        &self,
        tags: &[ReviewTag],
        empty: Option<Empty>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = theme::theme(cx);
        let preferences = Preferences::from_settings(crate::settings::current(cx));
        let mut content = div()
            .id("ai-tags")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .py(px(4.))
            .pr(px(ui::scrollbar::GUTTER))
            .flex()
            .flex_col();
        let empty = match (empty, self.active) {
            (Some(empty), _) => Some(empty),
            (None, None) => Some(Empty::NoSuggestions),
            (None, Some(_)) => None,
        };
        match (empty, self.active) {
            (Some(empty), _) => {
                let text: SharedString = if empty == Empty::Error {
                    self.error.clone().unwrap_or_default().into()
                } else {
                    empty.text().into()
                };
                content = content.child(
                    div()
                        .size_full()
                        .px(px(24.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .text(theme::TEXT_SM)
                        .text_center()
                        .text_color(if empty == Empty::Error {
                            theme.negative()
                        } else {
                            theme.text_tertiary()
                        })
                        .child(text),
                );
            }
            (None, Some(item)) => {
                for (index, tag) in tags.iter().enumerate() {
                    content = content.child(self.tag_row(index, item, tag, &preferences, cx));
                }
            }
            (None, None) => {}
        }
        let mut column = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(LIST))
            .min_w_0()
            .flex()
            .flex_col();
        if let Some(error) = self.error.clone().filter(|_| !tags.is_empty()) {
            column = column.child(
                div()
                    .px(px(10.))
                    .py(px(7.))
                    .border_b_1()
                    .border_color(theme.border_secondary())
                    .text(theme::TEXT_CAPTION)
                    .line_height(px(theme::TEXT_CAPTION * 1.35))
                    .text_color(theme.negative())
                    .child(error),
            );
        }
        column.child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(content)
                .child(ui::scrollbar(
                    "ai-tags-scrollbar",
                    &self.list_scroll,
                    window,
                    cx,
                )),
        )
    }
}

impl Render for AiTagger {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() {
            return div().into_any_element();
        }
        let theme = theme::theme(cx);
        if self.width_fade.animating() || self.rail_shown.animating() {
            window.request_animation_frame();
        }
        let width = self.width();
        let cutoff = self.cutoff();

        let prediction = self
            .active
            .and_then(|id| self.predictions.iter().find(|p| p.root_id == id));
        let all = review::prediction_tags(prediction, &self.run_models);
        let (suggested, below): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|tag| tag.confidence >= cutoff);
        let source = if self.view == ViewMode::Suggested {
            &suggested
        } else {
            &below
        };
        let visible = if self.query.trim().is_empty() {
            source.clone()
        } else {
            crate::text::rank_matches(source.clone(), &self.query, |tag| tag.key.as_str())
        };
        let checked =
            review::assignments(&self.predictions, &self.run_models, &self.overrides, cutoff)
                .iter()
                .map(|assignment| assignment.tags.len())
                .sum::<usize>();
        let this_running = self.running && self.progress.current == self.active;
        let empty = visible.is_empty().then(|| {
            Empty::of(&EmptyState {
                error: self.error.is_some(),
                running: self.running,
                resolved: self.resolved,
                items: self.items.len(),
                this_running,
                models: self.run_models.len(),
                below: self.view == ViewMode::Below,
                predictions: self.predictions.len(),
            })
        });

        let entity = cx.entity().downgrade();
        let (close, moved, nav, slid) = (
            entity.clone(),
            entity.clone(),
            entity.clone(),
            entity.clone(),
        );
        let search_handle = self.search.read(cx).focus_handle(cx);
        let search = div()
            .flex_1()
            .min_w_0()
            .h(px(36.))
            .pl(px(3.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.focus(&search_handle, cx);
            })
            .child(ui::icon("menu-search", 14., theme.text_secondary()).flex_none())
            .child(div().flex_1().min_w_0().h_full().child(self.search.clone()));
        let sidebar = self.sidebar;
        let toggle_rail = overlay_shell::header_button(
            "ai-sidebar",
            "picker-layout-sidebar",
            ui::tip(if sidebar {
                t("Hide sidebar")
            } else {
                t("Show sidebar")
            }),
        )
        .toggled(sidebar)
        .on_click(cx.listener(|this, _, _, cx| {
            this.sidebar = !this.sidebar;
            this.width_fade.set(this.sidebar, motion::NORMAL);
            this.rail_shown.set(
                this.sidebar,
                if this.sidebar {
                    motion::PICKER_RAIL_IN
                } else {
                    motion::PICKER_RAIL_OUT
                },
            );
            cx.notify();
        }));

        let mut body = div()
            .relative()
            .size_full()
            .child(self.rail(suggested.len(), below.len(), checked, window, cx))
            .child(self.review_pane(window, cx))
            .child(self.list(&visible, empty, window, cx));
        if self.running {
            // The hairline over the panes.
            body = body.child(div().absolute().top_0().left_0().right_0().child(
                ui::progress_bar().height(2.).filled(
                    "ai-progress",
                    ui::progress_bar::share(self.progress.done as i64, self.progress.total as i64),
                ),
            ));
        }

        let mut shell = overlay_shell::overlay_shell(
            "ai-tagger",
            self.geometry,
            width,
            HEIGHT,
            &self.focus,
            move |window, cx| {
                let _ = close.update(cx, |this, cx| this.close(window, cx));
            },
        )
        .pinned(self.pinned)
        .on_move(move |geometry, _, cx| {
            let _ = moved.update(cx, |this, cx| {
                this.geometry = geometry;
                cx.notify();
            });
        })
        .on_nav(move |key, _, cx| {
            nav.update(cx, |this, cx| this.navigate(key, cx))
                .unwrap_or(false)
        })
        .header(search);
        if self.running {
            shell = shell.header(
                div()
                    .flex_none()
                    .font_family(theme::FONT_MONO)
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .child(format!(
                        "{}{} {}{}",
                        t("Analyzing "),
                        (self.progress.done + 1).min(self.progress.total),
                        t("of "),
                        self.progress.total
                    )),
            );
        }
        let pin = overlay_shell::header_button(
            "ai-pin",
            if self.pinned {
                "pin-filled"
            } else {
                "picker-pin"
            },
            ui::tip(if self.pinned {
                t("Unpin")
            } else {
                t("Pin (keep open)")
            }),
        )
        .toggled(self.pinned)
        .on_click(cx.listener(|this, _, _, cx| {
            this.pinned = !this.pinned;
            cx.notify();
        }));
        shell = shell.header(toggle_rail).header(pin).child(body);

        let backend = match &self.backend {
            Some(backend) => format!("· {}", crate::tf!("{value0} inference", backend)),
            None => format!("· {}", t("Local inference")),
        };
        let confidence = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .text(theme::TEXT_CAPTION)
            .text_color(theme.text_tertiary())
            .child(t("Minimum confidence · All images"))
            .child(
                ui::slider(
                    "ai-confidence",
                    self.confidence as f64,
                    review::MIN_CONFIDENCE as f64,
                    review::MAX_CONFIDENCE as f64,
                    1.,
                )
                .width(88.)
                .on_change(move |value, cx| {
                    let _ = slid.update(cx, |this, cx| this.set_confidence(value, cx));
                }),
            )
            .child(
                div()
                    .min_w(px(28.))
                    .flex()
                    .justify_end()
                    .font_family(theme::FONT_MONO)
                    .text_color(theme.text_secondary())
                    .child(format!("{}%", self.confidence)),
            )
            .child(div().whitespace_nowrap().child(backend));
        let mut actions = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(overlay_shell::hint(cx).child(overlay_shell::kbd(t("Esc"), cx)));
        if checked > 0 {
            actions = actions.child(
                ui::action_button("ai-apply", Look::Primary)
                    .label(if self.applying {
                        t("Applying…").to_owned()
                    } else {
                        review::apply_label(checked)
                    })
                    .height(theme::CONTROL_SM)
                    .padding(12.)
                    .text_size(theme::TEXT_SM)
                    .disabled(self.applying || self.running)
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
            );
        }
        shell = shell.footer(confidence).footer(actions);
        div().child(shell).into_any_element()
    }
}
