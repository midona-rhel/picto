//! What the Tags screen draws. This file builds three things. The first is
//! the search field that the titlebar shows while the Tags screen is open
//! (`TagsToolbar`). The second is the rail on the left of the screen, which
//! has the rows All, General and Starred, one row for each tag group, and
//! the Delete Unused button. The third is the list of tags on the right. The
//! list has a heading for each first letter of the tags' names, a card for
//! each tag, and placeholders for the tags that are not loaded yet.
//!
//! The dialogs are built in `dialogs.rs` and the context menus in
//! `menus.rs`.
use super::dialogs::Dialog;
use super::model::{Row, Rows};
use super::{
    Browse, LOAD_AHEAD, RAIL_WIDTH, SEARCH_WIDTH, TagsScreen, TagsToolbar, ViewFade, dialogs, model,
};
use crate::i18n::t;
use crate::theme::Text as _;
use crate::ui::{self, field::Look};
use crate::{motion, theme};
use gpui::{prelude::FluentBuilder as _, *};
use picto_library::TagRecord;

impl Render for TagsToolbar {
    /// Builds the search field for tags, which the titlebar shows while the
    /// Tags screen is open. The field is 125 px wide (`SEARCH_WIDTH`). It
    /// has a 13 px search icon 7 px from its left edge. While it has text,
    /// it has a button at its right end that clears the text.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div().w(px(SEARCH_WIDTH)).flex_none().child(
            ui::field("tag-search", &self.search, Look::TitlebarSearch)
                .icon("library-search", 13., 7., theme.text_tertiary())
                .clearable()
                .clear_name(t("Clear tag search")),
        )
    }
}

impl TagsScreen {
    /// Builds one row of the rail (production's `.namespaceItem`). The row
    /// shows a 16 px icon in `icon_color`, then `label`, and `count` at its
    /// right end. `active` is true for the row whose tags the list shows.
    /// That row is filled with the theme's `surface_active` colour.
    /// Assistive technology reads the row as the label followed by the
    /// count. In that reading the count has the thousands separator of the
    /// system's language, for example "1,234" in English. The caller adds
    /// what a click on the row does.
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
        let name = format!("{label} {}", crate::i18n::format::grouped(count));
        ui::rail_item::rail_item(id, ui::rail_item::Look::Namespace, label, count.to_string())
            .accessible_name(name)
            .mark(ui::icon(icon, 16., icon_color).flex_none())
            .active(active)
            .build(cx)
    }

    /// Builds the rail: the column on the left of the screen, 184 px wide
    /// (`RAIL_WIDTH`), which scrolls and has a scrollbar. From top to bottom
    /// it has the rows All, General and Starred, the heading "Groups(n)"
    /// with the button that opens the Create tag group dialog, one row for
    /// each tag group, and the Delete Unused button. A click on a row makes
    /// the list show that row's tags (`TagsScreen::show`). A right-click on
    /// the row of a group opens that group's context menu.
    fn rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let showing = |browse: Browse| self.browse == browse;
        // The All row shows the number of tags in all groups together. The
        // General row shows the number of tags in the group whose name is
        // empty, which holds the tags that are in no named group.
        let total = self.namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        let general = self
            .namespaces
            .iter()
            .find(|ns| ns.name.is_empty())
            .map_or(0, |ns| ns.tag_count);
        // The groups that get a row of their own: every group whose name is
        // not empty and is not "general".
        let mut groups = self
            .namespaces
            .iter()
            .filter(|ns| !ns.name.is_empty() && ns.name != "general")
            .cloned()
            .collect::<Vec<_>>();
        // The rows are in the order of `crate::tags::group_order`: creator,
        // series, character, species, rating, and then every other group.
        // The sort is stable, so groups with the same place in that order
        // stay in the order in which the library returned them, which is by
        // name without regard to upper and lower case.
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
                    t("All").into(),
                    total,
                    showing(Browse::All),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::All, cx))),
            )
            .child(
                self.rail_item(
                    "tags-general",
                    "pin-bookmark",
                    theme.text(),
                    t("General").into(),
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
                    "star",
                    theme.text(),
                    t("Starred").into(),
                    self.preferences.starred.len() as u64,
                    showing(Browse::Starred),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::Starred, cx))),
            )
            .child(
                // The heading above the rows of the groups (production's
                // `.railHeading`). It shows the text "Groups(n)" in 12 px
                // tertiary text, where n is the number of groups that have
                // a row. At its right end is a 20 px button that opens the
                // Create tag group dialog. The button is disabled while an
                // edit of the library is running (`busy`).
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
                    .child(format!("{}{})", t("Groups("), groups.len()))
                    .child(
                        ui::icon_button("create-tag-group", "tags-plus")
                            .size(20.)
                            .glyph(15.)
                            .disabled(self.busy)
                            .tip(ui::tip(t("Create tag group")))
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
        // The Delete Unused button at the end of the rail (production's
        // `.cleanupTagsButton`). It shows a trash icon, the text "Delete
        // Unused" and, at its right end, the number of tags that are
        // assigned to no item (`unused`). It is enabled when that number is
        // more than 0 and no edit of the library is running. When it is
        // enabled, a click opens the dialog that asks whether to delete
        // those tags (`Dialog::Cleanup`), and while the pointer is over the
        // button, the button has the hover fill and its text and icon have
        // the theme's negative colour. When it is disabled, it is drawn at
        // 35% opacity.
        let enabled = self.unused > 0 && !self.busy;
        let mut cleanup = div()
            .id("delete-unused-tags")
            .role(Role::Button)
            .aria_label(format!(
                "{} {}",
                t("Delete Unused"),
                crate::i18n::format::grouped(self.unused)
            ))
            .aria_disabled(!enabled)
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
                ui::icon("trash-2", 14., theme.text_secondary())
                    .flex_none()
                    .group_hover("cleanup", move |style| {
                        if enabled {
                            style.text_color(theme.negative())
                        } else {
                            style
                        }
                    }),
            )
            .child(t("Delete Unused"))
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
                    .role(Role::Navigation)
                    .aria_label(t("Tag groups"))
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

    /// Builds the card of one tag in the list (production's `.tagCard`). The
    /// card is `width` pixels wide and 26 px tall (`model::ROW_HEIGHT`).
    /// From left to right it shows a 5 px dot in the colour of the tag's
    /// group, the tag's name without the name of its group, a 12 px star
    /// when the tag is starred, and in brackets the number of items the tag
    /// is assigned to. The dot has 50% opacity. While the pointer is over
    /// the card, the dot has 100% opacity and the card is filled with the
    /// theme's `surface_hover` colour. A click on the card opens the tag's
    /// editor. A right-click opens the tag's context menu.
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
        // `measure` returns the width in pixels of one line of `text` in the
        // font family `family` at `size` pixels.
        let measure = |text: SharedString, family: &'static str, size: f32, window: &mut Window| {
            let face = Font {
                family: family.into(),
                ..window.text_style().font()
            };
            ui::text_width(&text, face, size, window)
        };
        let label: SharedString = tag.subname.clone().into();
        // `room` is the width in pixels that the card has for the name. It
        // is the card's width less the 4 px of padding on each side, 18 px
        // for the dot with its margins, 12 px for the star when the tag is
        // starred, the 6 px margin before the count, and the width of the
        // count's text. A name that is not wider than `room` is drawn at
        // its own width. A name that is wider takes the width that is left
        // in the card and is cut short with an ellipsis.
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
        let name_cell = div().min_w_0().child(label.clone());
        let name_cell = if overflows {
            name_cell.flex_1().truncate()
        } else {
            name_cell.flex_none().whitespace_nowrap()
        };
        let (opened, menu_tag) = (tag.clone(), tag.clone());
        div()
            .id(("tag", tag.tag_id.0 as usize))
            .role(Role::Button)
            .aria_label(if starred {
                format!("{label} {count} {}", t("Starred"))
            } else {
                format!("{label} {count}")
            })
            // The card has a fixed width and neither grows nor shrinks, so
            // every card is as wide as one column, also in a row that has
            // fewer tags than the list has columns (production's
            // `grid-template-columns: repeat(columns, minmax(0, 1fr))`).
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
                // The star is drawn in the theme's text colour. Production's
                // CSS gives the star `color: var(--color-warning)`, but
                // production defines no variable of that name, so the star
                // there has the text colour of the card.
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

    /// Builds the placeholder that is drawn in place of one tag that is not
    /// loaded yet (production's `.tagSkeleton`). It is `width` pixels wide
    /// and 26 px tall (`model::ROW_HEIGHT`), with a corner radius of 6 px.
    /// Its fill is a pattern that goes from 3% contrast to 8% contrast and
    /// back to 3% over twice the placeholder's width. The pattern moves
    /// from left to right by its whole length every 800 ms
    /// (`motion::TAG_SHIMMER`), on the `ease-in-out` curve, and then starts
    /// again.
    ///
    /// `model::shimmer` returns the state of the pattern at the present
    /// moment: the place where the pattern is at its lightest or its
    /// darkest, as a fraction of the placeholder's width, and the strength
    /// of the fill at the left edge, at that place and at the right edge. A
    /// strength of 0 is drawn as 3% contrast and a strength of 1 as 8%
    /// contrast. The placeholder is painted as two rectangles that meet at
    /// that place. Each has a linear gradient between the strengths at its
    /// two ends.
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

    /// The list of tags, which takes the width the rail leaves. The number
    /// of columns depends on the list's width and the rows that are drawn
    /// depend on its height, so the list is built for its own bounds in the
    /// frame that lays it out (`ui::sized_view`).
    fn list(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::sized_view(cx.weak_entity(), Self::build_list)
            .flex_1()
            .min_w_0()
            .h_full()
    }

    /// Builds the list of tags for `bounds`, the list's position and size
    /// in window coordinates in this frame.
    fn build_list(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.bounds = bounds;
        let theme = theme::theme(cx);
        // `width` is the list's width less the 8 px at its right edge that
        // are kept for the scrollbar (`ui::scrollbar::GUTTER`).
        let width = (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.);
        let columns = model::column_count(width);
        let searching = !self.query.trim().is_empty();
        // `virtual_count` is the number of tags that the rows are built
        // for. When all tags are loaded, it is the number of loaded tags.
        // While the first page is loading, or the pager has a further page
        // to load, it is the number of tags the list is expected to hold
        // (`expected`), if that is more than the number of loaded tags. The
        // rows of the tags that are not loaded yet are placeholders.
        let virtual_count = if self.pager.cursor.is_some() || self.loading {
            self.pager.tags.len().max(self.expected())
        } else {
            self.pager.tags.len()
        };
        // The rows are built again only when one of these four has changed:
        // the loaded tags, the number of columns, `virtual_count`, and
        // whether the list shows the results of a search.
        let key = Some((self.tags_epoch, columns, virtual_count, searching));
        if key != self.rows_key {
            self.rows = Rows::build(&self.pager.tags, searching, columns, virtual_count);
            self.rows_key = key;
        }
        // `top` is the position of the viewport's top edge in pixels below
        // the top of the first row: the distance the list is scrolled, less
        // the 8 px of padding above the rows (`model::LIST_TOP`). `visible`
        // is the range of the rows that are in the viewport, with 12 more
        // rows before and after them (`model::OVERSCAN`).
        let top = -f32::from(self.scroll.offset().y) - model::LIST_TOP;
        let viewport = f32::from(self.bounds.size.height);
        let visible = self.rows.visible(top, top + viewport);
        // `load_boundary` is the index of the first placeholder row, or the
        // number of rows when there is no placeholder row. The next page of
        // tags is asked for when the last row in `visible` is 4 rows
        // (`LOAD_AHEAD`) or fewer before that index, or is at or after it.
        // The comparison is with the first placeholder row and not with
        // the last row of the list, because the placeholder rows stand for
        // tags that are not loaded: the next page has to be asked for as
        // soon as the first of them comes near the viewport.
        if visible.end > 0 && visible.end - 1 + LOAD_AHEAD >= self.rows.load_boundary() {
            self.load_more(cx);
        }

        // Each row is 12 px in from the left and the right edge of `width`,
        // so it does not reach into the space kept for the scrollbar. The
        // cards of a row are 2 px apart and all have the same width,
        // `card_width`.
        let row_width = width - 24.;
        let card_width = (row_width - 2. * (columns as f32 - 1.)) / columns as f32;
        let mut shimmering = false;
        // `content` is as tall as all the rows together, with 8 px of
        // padding above them and 24 px below them, so the list scrolls over
        // the height of all the rows. Only the rows in `visible` are built,
        // each at its own position in `content`. `content` fades in over
        // 90 ms (`motion::TAG_LIST_FADE`). The 90 ms are counted from
        // `list_shown`, which is the moment the first page of tags arrived
        // after the user chose another row of the rail or changed the
        // search text.
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
                // A heading, 42 px tall (`model::SECTION_HEIGHT`). It shows
                // its label in 14 px text. The label is the first character
                // of the names of the tags under the heading, in upper
                // case, or "#" when that character is not a letter or a
                // digit, or "Matches" above the results of a search. After
                // the label, in brackets and in 11 px tertiary text, is the
                // number of tags under the heading.
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
                // One row of tag cards. Under the last row of a heading
                // there is a line 1 px tall. The line is 4 px below the
                // row's bottom edge, so that a card's hover fill, which is
                // as tall as the row, does not touch it.
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
                                .bottom(px(-5.))
                                .h(px(1.))
                                .bg(theme.border_secondary()),
                        )
                    })
                }
                // One row of placeholders for tags that are not loaded yet,
                // with one placeholder in each column.
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
        // The animation of the placeholders repeats for as long as tags are
        // loading. While a placeholder row is built, the screen is
        // therefore drawn again every 33 ms (`motion::loop_frame`), and not
        // in every frame of the display. The fade-in of the rows lasts only
        // 90 ms, and during it the screen is drawn again in every frame.
        if shimmering {
            motion::loop_frame(window, cx);
        }
        if self.list_shown.elapsed() < motion::TAG_LIST_FADE {
            window.request_animation_frame();
        }

        // The text "No tags" is shown in the centre of the list when the
        // list has no tags and `loading` is false. `loading` is true from
        // the moment the tags of another row of the rail, or of a changed
        // search text, are asked for until their first page arrives.
        let empty = !self.loading && self.pager.tags.is_empty();
        div()
            .relative()
            .size_full()
            // The whole list, with its scrollbar, has the opacity of
            // `ViewFade`. It fades out over 100 ms when the user chooses
            // another row of the rail. It fades in over 140 ms when the
            // first page of tags has arrived for the new row, or for a
            // changed search text.
            .opacity(self.fade.opacity())
            .child(
                div()
                    .id("tag-list")
                    .role(Role::Region)
                    .aria_label(t("Tags"))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    // The screen is drawn again when the list scrolls, so
                    // that the rows for the new scroll position are built.
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
                    .child(t("No tags"))
            }))
            .child(ui::scrollbar(
                "tag-list-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
            .into_any_element()
    }
}

/// Returns the element that shows `count` at the right end of the Delete
/// Unused button, in 11 px tertiary text (production's `.namespaceCount`,
/// the class of every count in the rail).
fn count_label(count: u64, cx: &App) -> Div {
    div()
        .flex_none()
        .ml_auto()
        .text_color(theme::theme(cx).text_tertiary())
        .text(theme::TEXT_CAPTION)
        .child(count.to_string())
}

impl Render for TagsScreen {
    /// Builds the Tags screen. At the top is the message of an edit or a
    /// load that failed, when there is one. Below it are the rail on the
    /// left and the list of tags on the right. The open dialogs and the open
    /// context menu are drawn over them.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        // `advance_fade` moves the fade of the list to its next stage when
        // the present stage has lasted its full time. While the list is
        // fading out or fading in, the screen is drawn again in every
        // frame.
        self.advance_fade(cx);
        if matches!(self.fade, ViewFade::Leaving(_) | ViewFade::Entering(_)) {
            window.request_animation_frame();
        }
        // A dialog that the user dismissed fades out for 120 ms
        // (`motion::MODAL_OUT`). The dialogs whose fade-out has ended are
        // removed from `layers` here. When one was removed, or when
        // `refocus` was set since the last frame, the keyboard focus goes
        // to the topmost dialog that is left, or to the screen itself when
        // no dialog is left.
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
        // While a dialog is fading out, the screen is drawn again in every
        // frame, so that the dialog is removed as soon as its fade-out has
        // ended.
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
        let list = self.list(cx);
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
