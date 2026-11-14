//! The Tags screen's drawing: the toolbar's search field, the rail of
//! sections and groups, the list of tag cards with its letter headings and
//! loading skeletons.
use super::dialogs::Dialog;
use super::model::{Row, Rows};
use super::{
    Browse, LOAD_AHEAD, RAIL_WIDTH, SEARCH_WIDTH, TagsScreen, TagsToolbar, ViewFade, dialogs, model,
};
use crate::theme::Text as _;
use crate::ui::{self, field::Look};
use crate::{motion, theme};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::TagRecord;

impl Render for TagsToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div().w(px(SEARCH_WIDTH)).flex_none().child(
            ui::field("tag-search", &self.search, Look::TitlebarSearch)
                .icon("tags-search", 13., 7., theme.text_tertiary())
                .clearable(),
        )
    }
}

impl TagsScreen {
    /// `.namespaceItem`: one row of the rail.
    #[allow(clippy::too_many_arguments)]
    fn rail_item(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
        icon_color: Rgba,
        label: SharedString,
        count: u64,
        active: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        div()
            .id(id)
            .h(px(theme::CONTROL_SM))
            .mx(px(8.))
            .mb(px(1.))
            .px(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .when(active, |row| row.bg(theme.surface_active()))
            // `.namespaceItem:hover` outranks `.namespaceItemActive`.
            .hover(move |style| style.bg(theme.surface_hover()))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(theme::CONTROL_SM))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon(icon, 16., icon_color).flex_none())
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.text())
                            .child(label),
                    ),
            )
            .child(count_label(count, cx))
    }

    fn rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let showing = |browse: Browse| self.browse == browse;
        let total = self.namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        let general = self
            .namespaces
            .iter()
            .find(|ns| ns.name.is_empty())
            .map_or(0, |ns| ns.tag_count);
        let mut groups = self
            .namespaces
            .iter()
            .filter(|ns| !ns.name.is_empty() && ns.name != "general")
            .cloned()
            .collect::<Vec<_>>();
        // A stable sort: groups of one order keep the library's name order.
        groups.sort_by_key(|ns| crate::tags::group_order(&ns.name));

        let mut column = div()
            .pt(px(8.))
            .pb(px(8.))
            .pr(px(ui::scrollbar::GUTTER))
            .child(
                self.rail_item(
                    "tags-all",
                    "tags-bookmarks",
                    theme.text(),
                    "All".into(),
                    total,
                    showing(Browse::All),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::All, cx))),
            )
            .child(
                self.rail_item(
                    "tags-general",
                    "tags-bookmark",
                    theme.text(),
                    "General".into(),
                    general,
                    showing(Browse::Group(String::new())),
                    cx,
                )
                .on_click(
                    cx.listener(|this, _, _, cx| this.show(Browse::Group(String::new()), cx)),
                ),
            )
            .child(
                self.rail_item(
                    "tags-starred",
                    "tags-star",
                    theme.text(),
                    "Starred".into(),
                    self.preferences.starred.len() as u64,
                    showing(Browse::Starred),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::Starred, cx))),
            )
            .child(
                // `.railHeading`
                div()
                    .h(px(theme::CONTROL_SM))
                    .mt(px(12.))
                    .mx(px(8.))
                    .mb(px(3.))
                    .pl(px(3.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(theme.text_tertiary())
                    .text(theme::TEXT_SM)
                    .child(format!("Groups({})", groups.len()))
                    .child(
                        ui::icon_button("create-tag-group", "tags-plus")
                            .size(20.)
                            .glyph(15.)
                            .disabled(self.busy)
                            .tip(ui::tip("Create tag group"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let dialog = dialogs::create_group(cx);
                                this.push(dialog, cx);
                            })),
                    ),
            );
        for group in groups {
            let color = rgb(crate::tags::group_color(&group.name, &self.preferences));
            let active = showing(Browse::Group(group.name.clone()));
            let label = if group.name.is_empty() {
                "general".to_owned()
            } else {
                group.name.clone()
            };
            let (shown, opened) = (group.name.clone(), group.clone());
            column = column.child(
                self.rail_item(
                    ("tags-group", group.namespace_id.0 as usize),
                    "tags-bookmark-group",
                    color,
                    label.into(),
                    group.tag_count,
                    active,
                    cx,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.show(Browse::Group(shown.clone()), cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.open_group_menu(opened.clone(), event.position, window, cx);
                        cx.stop_propagation();
                    }),
                ),
            );
        }
        // `.cleanupTagsButton`
        let enabled = self.unused > 0 && !self.busy;
        let mut cleanup = div()
            .id("delete-unused-tags")
            .h(px(theme::CONTROL_SM))
            .mt(px(10.))
            .mx(px(8.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .text_color(theme.text_secondary())
            .text(theme::TEXT_SM)
            .line_height(px(theme::CONTROL_SM))
            .group("cleanup")
            .child(
                ui::icon("tags-trash", 14., theme.text_secondary())
                    .flex_none()
                    .group_hover("cleanup", move |style| {
                        if enabled {
                            style.text_color(theme.negative())
                        } else {
                            style
                        }
                    }),
            )
            .child("Delete Unused")
            .child(count_label(self.unused, cx));
        cleanup = if enabled {
            cleanup
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_hover()).text_color(theme.negative()))
                .on_click(cx.listener(|this, _, _, cx| this.push(Dialog::Cleanup, cx)))
        } else {
            cleanup.opacity(0.35)
        };
        column = column.child(cleanup);

        div()
            .relative()
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .bg(theme.background())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .id("tag-rail")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.rail_scroll)
                    .child(column),
            )
            .child(ui::scrollbar(
                "tag-rail-scrollbar",
                &self.rail_scroll,
                window,
                cx,
            ))
    }

    /// `.tagCard`: a tag in the list, with its group's dot, a star when
    /// starred, and how many items carry it.
    fn tag_card(
        &self,
        tag: &TagRecord,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let name = crate::tags::record_name(tag);
        let starred = self.preferences.is_starred(&name);
        let color = rgb(crate::tags::group_color(&tag.namespace, &self.preferences));
        let count = format!("({})", tag.assignment_count);
        // The name keeps its own width beside the count unless it is too
        // long, when it takes what is left and ends in an ellipsis.
        let measure = |text: SharedString, font: &'static str, size: f32, window: &mut Window| {
            let mut style = window.text_style();
            style.font_family = font.into();
            let run = style.to_run(text.len());
            f32::from(
                window
                    .text_system()
                    .shape_line(text, px(size), &[run], None)
                    .width,
            )
        };
        let label: SharedString = tag.subname.clone().into();
        let room = width
            - 8.
            - 18.
            - if starred { 12. } else { 0. }
            - 6.
            - measure(
                count.clone().into(),
                theme::FONT_INSPECTOR_MONO,
                theme::TEXT_CAPTION,
                window,
            );
        let overflows = measure(label.clone(), theme::FONT_UI, theme::TEXT_MD, window) > room;
        let name_cell = div().min_w_0().child(label);
        let name_cell = if overflows {
            name_cell.flex_1().truncate()
        } else {
            name_cell.flex_none().whitespace_nowrap()
        };
        let (opened, menu_tag) = (tag.clone(), tag.clone());
        div()
            .id(("tag", tag.tag_id.0 as usize))
            // `repeat(columns, minmax(0, 1fr))`: every column the same
            // width, however many tags the row has.
            .w(px(width))
            .flex_none()
            .h(px(model::ROW_HEIGHT))
            .px(px(4.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .group("tag-card")
            .hover(move |style| style.bg(theme.surface_hover()))
            .text(theme::TEXT_MD)
            .line_height(px(model::ROW_HEIGHT))
            .child(
                div()
                    .size(px(5.))
                    .flex_none()
                    .ml(px(4.))
                    .mr(px(9.))
                    .rounded_full()
                    .bg(color)
                    .opacity(0.5)
                    .group_hover("tag-card", |style| style.opacity(1.)),
            )
            .child(name_cell)
            .children(starred.then(|| {
                // `color: var(--color-warning)` names no token, so the star
                // takes the label's colour, as it does in the Electron app.
                ui::icon("tags-star-solid", 12., theme.text())
                    .flex_none()
                    .ml_auto()
            }))
            .child(
                div()
                    .flex_none()
                    .ml(px(6.))
                    .opacity(0.5)
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .text(theme::TEXT_CAPTION)
                    .child(count),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_editor(opened.clone(), None, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_tag_menu(menu_tag.clone(), event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    /// `.tagSkeleton`: a placeholder with the moving shimmer.
    fn skeleton(&self, width: f32, cx: &App) -> AnyElement {
        let theme = theme::theme(cx);
        let elapsed = self.clock.elapsed().as_secs_f32() % motion::TAG_SHIMMER.as_secs_f32();
        let eased = motion::EASE_IN_OUT.ease(elapsed / motion::TAG_SHIMMER.as_secs_f32());
        let (bend, values) = model::shimmer(eased);
        let strength = move |value: f32| theme.contrast(3. + 5. * value);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let radius = px(theme::RADIUS_MD);
                let split = bounds.left() + bounds.size.width * bend;
                let piece = |from: Pixels, to: Pixels| {
                    Bounds::from_corners(point(from, bounds.top()), point(to, bounds.bottom()))
                };
                let gradient = |left: f32, right: f32| {
                    linear_gradient(
                        90.,
                        linear_color_stop(strength(left), 0.),
                        linear_color_stop(strength(right), 1.),
                    )
                };
                if split > bounds.left() {
                    window.paint_quad(
                        fill(piece(bounds.left(), split), gradient(values[0], values[1]))
                            .corner_radii(Corners {
                                top_left: radius,
                                bottom_left: radius,
                                ..Default::default()
                            }),
                    );
                }
                if split < bounds.right() {
                    window.paint_quad(
                        fill(piece(split, bounds.right()), gradient(values[1], values[2]))
                            .corner_radii(Corners {
                                top_right: radius,
                                bottom_right: radius,
                                ..Default::default()
                            }),
                    );
                }
            },
        )
        .w(px(width))
        .flex_none()
        .h(px(model::ROW_HEIGHT))
        .into_any_element()
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let width = (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.);
        let columns = model::column_count(width);
        let searching = !self.query.trim().is_empty();
        let virtual_count = if self.pager.cursor.is_some() || self.loading {
            self.pager.tags.len().max(self.expected())
        } else {
            self.pager.tags.len()
        };
        let key = Some((self.tags_epoch, columns, virtual_count, searching));
        if key != self.rows_key {
            self.rows = Rows::build(&self.pager.tags, searching, columns, virtual_count);
            self.rows_key = key;
        }
        let top = -f32::from(self.scroll.offset().y) - model::LIST_TOP;
        let viewport = f32::from(self.bounds.size.height);
        let visible = self.rows.visible(top, top + viewport);
        // Fetch at the boundary of what is loaded, not at the end of the
        // placeholders' estimate.
        if visible.end > 0 && visible.end - 1 + LOAD_AHEAD >= self.rows.load_boundary() {
            self.load_more(cx);
        }

        // The rows sit 12px in from each side of the content, which stops
        // short of the scrollbar's gutter.
        let row_width = width - 24.;
        let card_width = (row_width - 2. * (columns as f32 - 1.)) / columns as f32;
        let mut shimmering = false;
        let mut content = div()
            .relative()
            .w_full()
            .h(px(model::LIST_TOP + self.rows.total + model::LIST_BOTTOM))
            .opacity(motion::EASE_DEFAULT.ease(motion::progress(
                self.list_shown.elapsed(),
                motion::TAG_LIST_FADE,
            )));
        for index in visible {
            let start = model::LIST_TOP + self.rows.starts[index];
            let place = |row: Div| {
                row.absolute()
                    .top(px(start))
                    .left(px(12.))
                    .right(px(12. + ui::scrollbar::GUTTER))
            };
            let row = match self.rows.rows[index].clone() {
                Row::Section { label, count } => place(div())
                    .h(px(model::SECTION_HEIGHT))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_color(theme.text())
                    .text(theme::TEXT_LG)
                    .line_height(px(model::SECTION_HEIGHT))
                    .child(label)
                    .child(
                        div()
                            .text_color(theme.text_tertiary())
                            .text(theme::TEXT_CAPTION)
                            .child(format!("({count})")),
                    ),
                Row::Tags { tags, section_end } => {
                    let mut row = place(div()).h(px(model::ROW_HEIGHT)).flex().gap(px(2.));
                    for position in tags {
                        let tag = self.rows.ordered[position].clone();
                        row = row.child(self.tag_card(&tag, card_width, window, cx));
                    }
                    row.when(section_end, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(1.))
                                .bg(theme.border_secondary()),
                        )
                    })
                }
                Row::Skeleton => {
                    shimmering = true;
                    let mut row = place(div()).h(px(model::ROW_HEIGHT)).flex().gap(px(2.));
                    for _ in 0..columns {
                        row = row.child(self.skeleton(card_width, cx));
                    }
                    row
                }
            };
            content = content.child(row);
        }
        if shimmering || self.list_shown.elapsed() < motion::TAG_LIST_FADE {
            window.request_animation_frame();
        }

        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                        // A notification made while a frame is being drawn
                        // does not by itself bring another frame.
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let empty = !self.loading && self.pager.tags.is_empty();
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .opacity(self.fade.opacity())
            .child(measure)
            .child(
                div()
                    .id("tag-list")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(content),
            )
            .children(empty.then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_MD)
                    .child("No tags")
            }))
            .child(ui::scrollbar(
                "tag-list-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}

/// `.namespaceCount`
fn count_label(count: u64, cx: &App) -> Div {
    div()
        .flex_none()
        .ml_auto()
        .text_color(theme::theme(cx).text_tertiary())
        .text(theme::TEXT_CAPTION)
        .child(count.to_string())
}

impl Render for TagsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        self.advance_fade(cx);
        if matches!(self.fade, ViewFade::Leaving(_) | ViewFade::Entering(_)) {
            window.request_animation_frame();
        }
        // Dialogs that have finished leaving go; the keyboard follows the
        // topmost one that is left.
        let before = self.layers.len();
        self.layers.retain(|layer| !layer.presence.finished());
        if self.layers.len() != before {
            self.refocus = true;
        }
        if std::mem::take(&mut self.refocus) {
            match self.layers.last() {
                Some(layer) => layer.focus_in(window, cx),
                None => window.focus(&self.focus, cx),
            }
        }
        // Until the last has gone, a dialog leaving keeps this repainting.
        if self.layers.iter().any(|layer| layer.presence.closing()) {
            window.request_animation_frame();
        }
        let menu = self.menu.layer(window, cx);

        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background())
            .text_color(theme.text())
            .track_focus(&self.focus);
        if let Some(error) = &self.error {
            root = root.child(dialogs::error_banner(error.clone(), cx));
        }
        let rail = self.rail(window, cx);
        let list = self.list(window, cx);
        root = root.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .bg(theme.background())
                .child(rail)
                .child(list),
        );
        for (index, layer) in self.layers.iter().enumerate() {
            let element = self.layer(index, layer, window, cx);
            root = root.child(element);
        }
        root.children(menu)
    }
}
