//! The tag picker's drawing: the rail of groups, the tag rows and the
//! create row, the list, the settings panel and the picker itself.
use super::model::display_name;
use super::{HEIGHT, LOAD_AHEAD, Layout, NARROW, RAIL, ROW, TagPicker, View};
use crate::i18n::t;
use crate::tags::{Preferences, record_name as name};
use crate::theme::Text as _;
use crate::ui::{
    self,
    check::{self, Mark},
    overlay_shell::{self},
};
use crate::{grid::format, motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::TagRecord;
use picto_library::predicate::SetMatchMode;

impl TagPicker {
    /// `.sidebarItem`: a 24px rail row, its badge brightening under the
    /// pointer or when chosen.
    fn rail_item(
        &self,
        id: &'static str,
        index: usize,
        mark: AnyElement,
        label: SharedString,
        count: u64,
        view: View,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let active = self.view == view;
        let group = SharedString::from(format!("{id}-{index}"));
        div()
            .id((id, index))
            .role(Role::Button)
            .aria_label(format!("{label} {}", format::grouped(count)))
            .aria_toggled(crate::ui::a11y::toggled(active))
            .group(group.clone())
            .h(px(24.))
            .flex_none()
            .pr(px(8.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_MD)
            .line_height(px(24.))
            .text_color(theme.text())
            .whitespace_nowrap()
            .bg(if active {
                theme.surface_active()
            } else {
                gpui::transparent_black().into()
            })
            .when_not_active(active, move |row| {
                row.hover(move |style| style.bg(theme.surface_hover()))
            })
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.))
                    .line_height(px(18.))
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .ml_auto()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .opacity(if active { 1. } else { 0.7 })
                    .group_hover(group, |style| style.opacity(1.))
                    .child(format::grouped(count)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.show_view(view.clone(), cx)))
    }

    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let total = self.namespaces.iter().map(|group| group.tag_count).sum();
        let bookmark = |filled: bool| {
            ui::icon(
                if filled {
                    "picker-rail-bookmark-filled"
                } else {
                    "pin-bookmark"
                },
                12.,
                theme.text_secondary(),
            )
            .flex_none()
            .mx(px(5.))
            .into_any_element()
        };
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
            .cursor_pointer()
            .opacity(self.rail_shown.value_with(motion::EASE))
            .child(self.rail_item(
                "rail-all",
                0,
                bookmark(self.view == View::All),
                t("All").into(),
                total,
                View::All,
                cx,
            ));
        if settings.show_tag_groups {
            for (index, group) in crate::tags::rail_groups(&self.namespaces, false)
                .into_iter()
                .enumerate()
            {
                let view = View::Group(group.name.clone());
                let hovered = SharedString::from(format!("rail-group-{index}"));
                let dot = div()
                    .size(px(10.))
                    .flex_none()
                    .mx(px(6.))
                    .rounded_full()
                    .bg(rgb(crate::tags::group_color(&group.name, &preferences)))
                    .opacity(if self.view == view { 1. } else { 0.85 })
                    .group_hover(hovered, |style| style.opacity(1.))
                    .into_any_element();
                let label = if group.name.is_empty() {
                    "general".to_owned()
                } else {
                    group.name.clone()
                };
                rail = rail.child(self.rail_item(
                    "rail-group",
                    index,
                    dot,
                    label.into(),
                    group.tag_count,
                    view,
                    cx,
                ));
            }
        }
        rail.child(
            div()
                .h(px(1.))
                .my(px(4.))
                .flex_none()
                .bg(theme.border_secondary()),
        )
        .child(self.rail_item(
            "rail-selected",
            0,
            bookmark(self.view == View::Selected),
            t("Selected").into(),
            self.entity.len() as u64,
            View::Selected,
            cx,
        ))
        .child(
            self.rail_item(
                "rail-starred",
                0,
                ui::icon("star", 10., theme.text())
                    .flex_none()
                    .mx(px(6.))
                    .into_any_element(),
                t("Starred").into(),
                settings.starred_tags.len() as u64,
                View::Starred,
                cx,
            ),
        )
    }

    /// `.tagRow`
    fn row(
        &self,
        index: usize,
        tag: &TagRecord,
        preferences: &Preferences,
        prefixes: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let tag_name = name(tag);
        let checked = self.selected.contains(&tag_name);
        let excluded = self
            .filter
            .as_ref()
            .is_some_and(|filter| filter.excluded.contains(&tag_name));
        // `.tagRowExcluded`: the name, mark and count fade.
        let dim = if excluded { 0.45 } else { 1. };
        let focused = self.focus_index == Some(index);
        let starred = preferences.is_starred(&tag_name);
        let color = crate::tags::group_color(&tag.namespace, preferences);
        let group = SharedString::from(format!("picker-row-{index}"));
        let fill = if checked {
            theme.primary_at(20.)
        } else if focused {
            theme.surface_active()
        } else {
            gpui::transparent_black().into()
        };
        let outline = match (checked, focused) {
            (true, true) => Some(theme.primary_at(50.)),
            (false, true) => Some(theme.contrast(25.)),
            _ => None,
        };
        let mark = if starred {
            ui::icon("star", 10., theme.text()).flex_none()
        } else {
            ui::icon(
                if checked {
                    "picker-bookmark-filled"
                } else {
                    "filter-bookmark"
                },
                14.,
                rgb(color),
            )
            .flex_none()
        };
        let label = display_name(tag, prefixes);
        // A check box named by the tag and its count: ticked while the tag
        // is chosen, mixed while a filter excludes it; the row the keys are
        // on reads as focused.
        let name = format!("{label} ({})", format::grouped(tag.active_count));
        let text = overlay_shell::search_match(label, query, cx);
        let row_tag = tag.clone();
        let menu_tag = tag.clone();
        let mut row = div()
            .id(("picker-tag", index))
            .role(Role::CheckBox)
            .aria_label(name)
            .aria_toggled(if excluded {
                Toggled::Mixed
            } else {
                crate::ui::a11y::toggled(checked)
            })
            .when(focused, |row| row.aria_active_descendant())
            .group(group.clone())
            .relative()
            .h(px(ROW))
            .min_w_0()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .bg(fill);
        if self.filter.is_some() {
            row = row.child(check::check_box(
                if excluded {
                    Mark::Excluded
                } else if checked {
                    Mark::Included
                } else {
                    Mark::Empty
                },
                check::Size::Compact,
            ));
        }
        row = row.child(mark.opacity(dim)).child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .opacity(dim)
                .text_color(theme::blend(theme.text(), rgb(color), 0.12))
                .child(text),
        );
        if !checked && !focused {
            row = row.hover(move |style| style.bg(theme.surface_hover()));
        }
        if self.counts {
            row = row.child(
                div()
                    .flex_none()
                    .ml_auto()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .opacity(0.7 * dim)
                    .group_hover(group, move |style| style.opacity(dim))
                    .child(format!("({})", format::grouped(tag.active_count))),
            );
        }
        if let Some(outline) = outline {
            row = row.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(outline),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| {
            this.focus_index = Some(index);
            this.toggle(&row_tag, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.filter.is_some() {
                    this.exclude(&menu_tag, cx);
                } else {
                    this.open_menu(menu_tag.clone(), event.position, window, cx);
                }
            }),
        )
    }

    /// `.createTagRow`: Create "name".
    fn create_row(&self, tag: &TagRecord, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let focused = self.focus_index == Some(0);
        let label = name(tag);
        let created = tag.clone();
        let mut row = div()
            .id("picker-create")
            .relative()
            .h(px(ROW))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .text_color(theme.text())
            .bg(if focused {
                theme.surface_active()
            } else {
                theme.glass()
            })
            .child(ui::icon("plus", 10., theme.text_tertiary()).opacity(0.65))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(t("Create \""))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .border_b_1()
                            .border_color(theme.contrast(20.))
                            .h(px(18.))
                            .line_height(px(18.))
                            .child(label),
                    )
                    .child("\""),
            );
        if focused {
            row = row.child(
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
        row.on_click(cx.listener(move |this, _, _, cx| this.toggle(&created, cx)))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let (create, rows) = self.sources_rows(&settings.starred_tags);
        let local = matches!(self.view, View::Selected | View::Starred);
        let shown_query = if local {
            self.query.clone()
        } else {
            self.pager
                .settled
                .as_ref()
                .map_or(String::new(), |(query, _)| query.clone())
        };
        let can_show_empty = local || self.pager.settled.is_some();
        // Near the end of what is loaded, the next page.
        let max = f32::from(self.scroll.max_offset().y);
        let scrolled = -f32::from(self.scroll.offset().y);
        if max - scrolled < LOAD_AHEAD {
            self.load_more(cx);
        }
        let mut content = div().w_full().p(px(4.)).flex().flex_col();
        let offset = create.is_some() as usize;
        if let Some(create) = &create {
            content = content.child(self.create_row(create, cx));
        }
        let columns = self.columns();
        for (chunk_index, chunk) in rows.chunks(columns).enumerate() {
            let mut line = div().w_full().h(px(ROW)).flex_none().flex();
            for (column, tag) in chunk.iter().enumerate() {
                let index = offset + chunk_index * columns + column;
                line = line.child(div().flex_1().min_w_0().child(self.row(
                    index,
                    tag,
                    &preferences,
                    settings.show_tag_prefixes,
                    &shown_query,
                    cx,
                )));
            }
            for _ in chunk.len()..columns {
                line = line.child(div().flex_1().min_w_0());
            }
            content = content.child(line);
        }
        let empty = rows.is_empty() && create.is_none();
        let scroller = div()
            .id("picker-tags")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER))
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()));
        let scroller = if empty && can_show_empty {
            scroller.child(
                div()
                    .h(px(60.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(if self.view == View::Selected {
                        t("No tags on this entity")
                    } else {
                        t("No tags found")
                    }),
            )
        } else if empty {
            scroller
        } else {
            scroller.child(content)
        };
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(NARROW))
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(if self.sidebar {
                theme.border_secondary()
            } else {
                gpui::transparent_black().into()
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(scroller)
                    .child(ui::scrollbar(
                        "picker-tags-scrollbar",
                        &self.scroll,
                        window,
                        cx,
                    )),
            )
    }

    /// The layout and counts settings (`.settingsPanel`).
    fn settings_panel(&self, cx: &mut Context<Self>) -> [AnyElement; 2] {
        let theme = theme::theme(cx);
        let layout = self.layout;
        let picked = cx.entity().downgrade();
        let tabs = overlay_shell::view_tabs(
            "picker-layout",
            &[
                ("picker-layout-list", t("List")),
                ("picker-layout-grid", t("Grid")),
            ],
            if layout == Layout::List { 0 } else { 1 },
            move |index, _, cx| {
                let _ = picked.update(cx, |this, cx| {
                    this.layout = if index == 0 {
                        Layout::List
                    } else {
                        Layout::Grid
                    };
                    this.focus_index = None;
                    this.keep_layout(cx);
                    cx.notify();
                });
            },
        );
        let row = || {
            div()
                .w_full()
                .min_h(px(32.))
                .px(px(4.))
                .py(px(2.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .rounded(px(theme::RADIUS_MD))
                .text_color(theme.text())
        };
        let backdrop = div()
            .id("picker-settings-backdrop")
            .role(Role::Button)
            .aria_label(t("Close tag picker settings"))
            .absolute()
            .inset_0()
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_open = false;
                cx.notify();
            }));
        let panel = ui::glass_panel("picker-settings", theme::RADIUS_LG, cx)
            .occlude()
            .absolute()
            .top(px(32.))
            .right(px(32.))
            .w(px(240.))
            .p(px(16.))
            .child(row().child(t("Layout")).child(tabs))
            .child(
                row()
                    .id("picker-counts")
                    .role(Role::CheckBox)
                    .aria_label(t("Show counts"))
                    .aria_toggled(crate::ui::a11y::toggled(self.counts))
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_hover()))
                    .child(t("Show counts"))
                    .child(check::check_box(
                        if self.counts {
                            Mark::Checked
                        } else {
                            Mark::Empty
                        },
                        check::Size::Compact,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.counts = !this.counts;
                        this.keep_layout(cx);
                        cx.notify();
                    })),
            );
        [backdrop.into_any_element(), panel.into_any_element()]
    }
}

/// `when` for a condition that leaves the row as it is.
trait WhenNotActive: Sized {
    fn when_not_active(self, active: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if active { self } else { then(self) }
    }
}
impl WhenNotActive for Stateful<Div> {}

impl Render for TagPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu.layer(window, cx);
        if !self.is_open() {
            return div().into_any_element();
        }
        let theme = theme::theme(cx);
        if self.rail_width.animating() || self.rail_shown.animating() {
            window.request_animation_frame();
        }
        let width = self.width();
        let entity = cx.entity().downgrade();
        let (moved, nav) = (entity.clone(), entity.clone());
        let close = entity.clone();
        let sidebar = self.sidebar;
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
        let toggle_rail = overlay_shell::header_button(
            "picker-sidebar",
            "picker-layout-sidebar",
            ui::tip(if sidebar {
                t("Hide sidebar")
            } else {
                t("Show sidebar")
            }),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.sidebar = !this.sidebar;
            this.rail_width.set(this.sidebar, motion::NORMAL);
            this.rail_shown.set(
                this.sidebar,
                if this.sidebar {
                    motion::PICKER_RAIL_IN
                } else {
                    motion::PICKER_RAIL_OUT
                },
            );
            if !this.sidebar && !matches!(this.view, View::All) {
                this.view = View::All;
                this.request(cx);
            }
            cx.notify();
        }));
        let settings = overlay_shell::header_button(
            "picker-settings-button",
            "picker-adjustments",
            ui::tip(t("Tag picker settings")),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.settings_open = !this.settings_open;
            cx.notify();
        }));
        let pin = overlay_shell::header_button(
            "picker-pin",
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
        .on_click(cx.listener(|this, _, _, cx| {
            this.pinned = !this.pinned;
            cx.notify();
        }));

        let mut body = div()
            .relative()
            .size_full()
            .child(self.rail(cx))
            .child(self.list(window, cx));
        if self.settings_open && self.filter.is_none() {
            body = body.children(self.settings_panel(cx));
        }

        let mut footer_move = overlay_shell::hint(cx)
            .child(t("Move "))
            .child(overlay_shell::kbd("↑", cx))
            .child(overlay_shell::kbd("↓", cx));
        if self.layout == Layout::Grid {
            footer_move = footer_move
                .child(overlay_shell::kbd("←", cx))
                .child(overlay_shell::kbd("→", cx));
        }
        footer_move = footer_move
            .child(t("Select "))
            .child(overlay_shell::kbd("↵", cx));
        let mut shell = overlay_shell::overlay_shell(
            "tag-picker",
            self.geometry,
            width,
            HEIGHT,
            &self.focus,
            move |window, cx| {
                let _ = close.update(cx, |this, cx| this.close(window, cx));
            },
        )
        .pinned(self.pinned)
        .layer(self.layer)
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
        if let Some(filter) = &self.filter {
            // The match rule in place of the settings and the pin.
            let modes = [SetMatchMode::Any, SetMatchMode::All, SetMatchMode::Exact];
            let logic = cx.entity().downgrade();
            shell = shell
                .header(
                    ui::segmented::segmented(
                        "tag-picker-logic",
                        ui::segmented::Look::Logic,
                        vec![
                            ui::segmented::segment("logic-any", t("Match any")),
                            ui::segmented::segment("logic-all", t("Match all")),
                            ui::segmented::segment("logic-exact", t("Match exactly")),
                        ],
                        modes.iter().position(|mode| *mode == filter.mode),
                    )
                    .name(t("Filter matching rule"))
                    .on_select(move |index, _, cx| {
                        let _ = logic.update(cx, |this, cx| this.set_mode(modes[index], cx));
                    }),
                )
                .header(toggle_rail)
                .child(body)
                .footer(
                    overlay_shell::hint(cx)
                        .child(t("Select"))
                        .child(overlay_shell::kbd(t("L-Click"), cx)),
                )
                .footer(
                    overlay_shell::hint(cx)
                        .child(t("Exclude"))
                        .child(overlay_shell::kbd(t("R-Click"), cx)),
                );
        } else {
            shell = shell
                .header(toggle_rail)
                .header(settings)
                .header(div().ml_auto().flex().child(pin))
                .child(body);
            if self.sidebar {
                shell = shell.footer(
                    overlay_shell::hint(cx)
                        .child(t("Switch "))
                        .child(overlay_shell::tab_key(cx)),
                );
            }
            shell = shell.footer(footer_move);
        }
        shell = shell.footer(
            overlay_shell::hint(cx)
                .child(t("Close "))
                .child(overlay_shell::kbd(t("Esc"), cx)),
        );
        div().child(shell).children(menu).into_any_element()
    }
}
