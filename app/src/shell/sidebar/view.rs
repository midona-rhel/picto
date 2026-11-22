//! This file draws the sidebar: the library switcher, the system rows, the
//! Quick Access section, and the Folders and Smart Folders sections with
//! their rows.
use super::{
    DoubleClick, Preferences, QUICK_ACCESS, RETURN_TO_TOP_AFTER, SECTIONS, Section, Sidebar,
    Switcher, keyboard, section_of,
};
use crate::backend::{Navigation, Screen};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::{self, field::Look, sidebar_row::Branch};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

impl Sidebar {
    /// Draws the library switcher. It shows the library's cover, the
    /// library's name and a selector glyph. Behind them is a highlight in
    /// the shape of a circle around the cover joined to a bar. The highlight
    /// is shown while the pointer is over the switcher or the library
    /// popover is open. A click opens the library popover, or closes it when
    /// it is open. While the popover is open, the highlight has the theme's
    /// `surface_active` colour instead of `surface_hover`, and the selector
    /// glyph keeps the colour it has while the pointer is over the switcher.
    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (cover, name, copy) = {
            let session = self.session.read(cx);
            let name = session
                .library_name()
                .unwrap_or_else(|| t("No library").into());
            (session.library_cover.clone(), name, session.disposable())
        };
        let open = self.library_popover.read(cx).is_open();
        let state = window.use_keyed_state("library-switcher", cx, |_, _| Switcher::default());
        let hint = window.use_keyed_state("library-switcher-tip", cx, |_, _| {
            ui::tooltip::Hover::default()
        });
        state.update(cx, |state, _| {
            state.lit.set(state.hovered || open, motion::FAST);
            state.open.set(open, motion::FAST);
        });
        let (lit, active) = {
            let state = state.read(cx);
            (state.lit.value(), state.open.value())
        };
        if state.read(cx).lit.animating()
            || state.read(cx).open.animating()
            || hint.read(cx).animating()
        {
            window.request_animation_frame();
        }
        // The cover is pinned in the thumbnail cache, so that the cache keeps
        // the cover's decoded image however far the grid scrolls.
        let pinned = cover.clone().map(|path| Resource::Path(path.into()));
        self.thumbnails
            .update(cx, |thumbnails, _| thumbnails.pin("library-cover", pinned));
        // The highlight is painted in a canvas, and a canvas cannot be given
        // an opacity. The highlight therefore fades in and out by blending
        // its fill colour with transparent black.
        let fill = theme::blend(
            gpui::transparent_black().into(),
            theme::blend(theme.surface_hover(), theme.surface_active(), active),
            lit,
        );
        let bounds = self.switcher.clone();
        let highlight = canvas(
            // The canvas fills the switcher, so its bounds are the switcher's
            // bounds. They are stored in `self.switcher`, and the library
            // popover opens under them.
            move |button, _, _| bounds.set(button),
            move |bounds, _, window, _| {
                // The highlight is two shapes joined: a circle of radius 24px
                // centred on the cover, and a bar 30px tall that runs from
                // the circle's centre to the switcher's right edge. The path
                // is the outline of the two together, so that the area where
                // they overlap is not painted twice. `meet` is the distance
                // from the switcher's left edge to the points where the
                // bar's top and bottom edges cross the circle.
                let (left, top, right) = (bounds.left(), bounds.top(), bounds.right());
                let meet = px(24. + (24f32 * 24. - 15. * 15.).sqrt());
                let corner = point(px(6.), px(6.));
                let mut path = PathBuilder::fill();
                path.move_to(point(left + meet, top + px(9.)));
                path.line_to(point(right - px(6.), top + px(9.)));
                path.arc_to(corner, px(0.), false, true, point(right, top + px(15.)));
                path.line_to(point(right, top + px(33.)));
                path.arc_to(
                    corner,
                    px(0.),
                    false,
                    true,
                    point(right - px(6.), top + px(39.)),
                );
                path.line_to(point(left + meet, top + px(39.)));
                path.arc_to(
                    point(px(24.), px(24.)),
                    px(0.),
                    true,
                    true,
                    point(left + meet, top + px(9.)),
                );
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            },
        )
        .absolute()
        .inset_0();
        let entered = (state, hint.clone());
        // The switcher is a button to assistive technology. Its label is the
        // library's name, and it is reported as expanded while the library
        // popover is open.
        let open = self.library_popover.read(cx).is_open();
        div()
            .id("library-switcher")
            .role(Role::Button)
            .aria_label(name.clone())
            .aria_expanded(open)
            .relative()
            .h(px(48.))
            .flex_none()
            .mt(px(4.))
            .mb(px(8.))
            .ml(px(theme::SIDEBAR_INSET))
            .mr(px(theme::SIDEBAR_INSET + 2.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                entered.0.update(cx, |state, cx| {
                    state.hovered = *hovered;
                    cx.notify();
                });
                entered.1.update(cx, |hint, cx| hint.set(*hovered, cx));
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.library_popover
                    .update(cx, |popover, cx| popover.toggle(window, cx));
            }))
            .child(highlight)
            .child(div().relative().ml(px(4.5)).flex_none().child(
                ui::library_avatar("library-switcher-avatar", cover, 39.).cache(&self.thumbnails),
            ))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .line_height(px(24.))
                    .child(name),
            )
            // When the open library is a disposable copy, a "Library copy"
            // badge is shown after the library's name, so that the user does
            // not mistake the copy for the library it was copied from.
            .children(copy.then(|| {
                div()
                    .relative()
                    .flex_none()
                    .px(px(5.))
                    .rounded(px(theme::RADIUS_SM))
                    .border_1()
                    .border_color(theme.border_secondary())
                    .text(theme::TEXT_XS)
                    .line_height(px(14.))
                    .text_color(theme.text_tertiary())
                    .child(en("Library copy"))
            }))
            .child(
                ui::icon(
                    "selector",
                    14.,
                    theme::blend(theme.text_tertiary(), theme.text(), lit),
                )
                .relative()
                .flex_none()
                .opacity(0.7 + 0.3 * lit),
            )
            .children(hint.read(cx).layer(&ui::tip(t("Switch Library")), 48., cx))
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs = Preferences::current(cx);
        crate::stats::rendered(crate::stats::View::Sidebar);
        // On the first draw the sidebar subscribes to the folder picker,
        // whose `MoveTo` event moves the folders chosen with "Move to..."
        // into the picked folder, and to the grid's drag model, so that
        // `grid_drag` follows every drag of items from the grid.
        if self._drags.is_none() {
            self._subscriptions.push(cx.subscribe_in(
                &self.folder_picker,
                window,
                |this, _, event: &crate::inspector::folder_picker::Event, window, cx| {
                    if let crate::inspector::folder_picker::Event::MoveTo(parent) = event {
                        this.move_folders_to(*parent, window, cx);
                    }
                },
            ));
            let drags = crate::grid::drag::model(cx);
            self._drags = Some(cx.subscribe_in(
                &drags,
                window,
                |this, _, event: &crate::grid::drag::DragEvent, window, cx| {
                    this.grid_drag(event, window, cx)
                },
            ));
        }
        self.bounds.clear();
        self.shown_rows.clear();
        // When keyboard focus leaves the rows, `keyboard_row` is cleared, so
        // that no row is drawn as focused.
        if self._rows_focus.is_none() {
            self._rows_focus = Some(cx.on_focus_out(&self.rows_focus, window, |this, _, _, cx| {
                this.keyboard_row = None;
                cx.notify();
            }));
        }
        let dragging = self.grid_dragging || self.node_drag.is_some();
        let theme = theme::theme(cx);
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        let filtering = !filter.is_empty();
        let (entries, active) = {
            let session = self.session.read(cx);
            (session.nav.clone(), session.scope)
        };
        let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
        let section =
            |index: usize| section_of(&entries[index].scope, entries[index].screen.is_some());
        let count_of = |wanted| {
            (0..entries.len())
                .filter(|index| section(*index) == Some(wanted))
                .count()
        };
        // While the filter field has text, `kept` marks each folder and smart
        // folder whose name contains the text, ignoring case, and each of
        // its ancestors. Only those rows are drawn, and every branch is
        // drawn expanded. System rows are not filtered.
        let mut kept = vec![!filtering; entries.len()];
        if filtering {
            for index in 0..entries.len() {
                if section(index).is_none() {
                    kept[index] = true;
                } else if entries[index].label.to_lowercase().contains(&filter) {
                    let mut at = Some(index);
                    while let Some(index) = at {
                        kept[index] = true;
                        at = entries[index].parent;
                    }
                }
            }
        }
        let shown_at = |index: usize| {
            if !kept[index] {
                return false;
            }
            if filtering {
                return true;
            }
            let mut parent = entries[index].parent;
            while let Some(ancestor) = parent {
                if self.collapsed.contains(&ids[ancestor]) {
                    return false;
                }
                parent = entries[ancestor].parent;
            }
            true
        };
        // Returns whether the row at `index` has a sibling below it, in the
        // same section, that the filter keeps.
        let has_next_sibling = |index: usize| {
            (index + 1..entries.len()).any(|other| {
                entries[other].parent == entries[index].parent
                    && section(other) == section(index)
                    && kept[other]
            })
        };

        // To assistive technology the list of destinations is a tree, and
        // each row is a tree item at its depth.
        let mut list = div()
            .id("navigation")
            .role(Role::Tree)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .track_focus(&self.rows_focus)
            .key_context("SidebarRows")
            .on_action(
                cx.listener(|this, _: &keyboard::FocusNext, _, cx| this.focus_step(true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &keyboard::FocusPrevious, _, cx| this.focus_step(false, cx)),
            )
            .on_action(cx.listener(|this, _: &keyboard::OpenFocused, _, cx| this.open_focused(cx)))
            .on_action(
                cx.listener(|this, _: &keyboard::LeaveRows, window, cx| {
                    this.leave_rows(window, cx)
                }),
            )
            .pr(px(ui::scrollbar::GUTTER + theme::SIDEBAR_LIST_PADDING))
            .pb(px(16.));

        // `activity` is the state of the subscriptions. While a subscription
        // is running or all subscriptions are paused, the Subscriptions row
        // shows a button that pauses all subscriptions, or resumes them when
        // they are paused.
        let activity = self.session.read(cx).subscriptions;
        // This loop adds the system rows (All, Inbox, Trash and the others in
        // `SYSTEM_ROWS`). It leaves out the rows that the user has switched
        // off in the visibility menu.
        for (index, entry) in entries.iter().enumerate() {
            if section(index).is_some() || !prefs.visible_system.contains(ids[index].as_str()) {
                continue;
            }
            let row = ui::sidebar_row(
                ElementId::Name(ids[index].clone().into()),
                entry.icon.clone().unwrap_or_default(),
                entry.label.clone(),
            )
            .count(entry.count.filter(|_| prefs.show_counts))
            .activity(
                (entry.screen == Some(Screen::Subscriptions) && activity.shown()).then(|| {
                    let session = self.session.clone();
                    ui::sidebar_row::Activity {
                        paused: activity.paused,
                        busy: activity.busy,
                        label: if activity.paused {
                            en("Resume all subscriptions").into()
                        } else {
                            en("Pause all subscriptions").into()
                        },
                        on_click: Box::new(move |_, _, cx| {
                            session.update(cx, |session, cx| session.toggle_all_subscriptions(cx))
                        }),
                    }
                }),
            )
            .active(active == index)
            .focused(self.keyboard_row.as_ref() == Some(&ids[index]))
            .drop_highlight(self.grid_drop.as_ref() == Some(&ids[index]))
            .dragging(dragging)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selection.clear();
                this.session
                    .update(cx, |session, cx| session.navigate(index, cx));
                cx.notify();
            }))
            .on_secondary(cx.listener(
                move |this, event: &MouseDownEvent, window, cx| {
                    this.system_menu(index, event.position, window, cx)
                },
            ));
            self.shown_rows.push(ids[index].clone());
            list = list.child(
                div()
                    .relative()
                    .child(row)
                    .child(self.bounds.recorder(ids[index].clone())),
            );
        }

        // The Quick Access section lists the folders and smart folders that
        // the user added to Quick Access, in the order the setting stores
        // them. It is drawn only when it has at least one row.
        let quick = self
            .quick_access(cx)
            .iter()
            .filter_map(|id| ids.iter().position(|known| known == id))
            .filter(|index| section(*index).is_some())
            .collect::<Vec<_>>();
        if prefs.show_quick_access && !quick.is_empty() {
            let expanded = !self.collapsed.contains(QUICK_ACCESS);
            list = list.child(
                ui::section_header("quick-access", t("Quick Access"))
                    .count(if prefs.show_counts { quick.len() } else { 0 })
                    .expanded(expanded)
                    .on_toggle(
                        cx.listener(|this, _, _, cx| this.toggle_collapsed(QUICK_ACCESS, cx)),
                    ),
            );
            if expanded {
                list = list.child(div().h(px(2.)));
                for index in quick {
                    let key = format!("quick-{}", ids[index]);
                    self.shown_rows.push(key.clone());
                    list = list.child(
                        div()
                            .relative()
                            .child(self.quick_row(index, &entries[index], active == index, cx))
                            .child(self.bounds.recorder(key)),
                    );
                }
            }
        }

        let mut matches = 0;
        for which in SECTIONS {
            if !prefs.shows(which) {
                self.rendered[which as usize].clear();
                continue;
            }
            let expanded = filtering || !self.collapsed.contains(which.id());
            let (tip, element) = match which {
                Section::Folders => (
                    ui::tip(t("New Folder")).shortcut("file.newFolder", cx),
                    "folders",
                ),
                Section::SmartFolders => {
                    (ui::tip(en("New Smart Folder or Group")), "smart-folders")
                }
            };
            list = list.child(
                ui::section_header(element, which.label())
                    .count(if prefs.show_counts {
                        count_of(which)
                    } else {
                        0
                    })
                    .expanded(expanded)
                    .on_toggle(cx.listener(move |this, _, _, cx| {
                        let filtering = !this.filter.read(cx).value().trim().is_empty();
                        if !filtering {
                            this.toggle_collapsed(which.id(), cx);
                        }
                    }))
                    .on_add(
                        tip,
                        cx.listener(
                            move |this, bounds: &Bounds<Pixels>, window, cx| match which {
                                Section::Folders => this.create_folder(None, window, cx),
                                // The add button of the Smart Folders section
                                // opens the section's menu at the point 4px
                                // below the button's bottom left corner.
                                Section::SmartFolders => this.section_menu(
                                    which,
                                    point(bounds.left(), bounds.bottom() + px(4.)),
                                    window,
                                    cx,
                                ),
                            },
                        ),
                    )
                    .on_secondary(
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.section_menu(which, event.position, window, cx)
                        }),
                    ),
            );
            let mut rendered = Vec::new();
            if expanded {
                for (index, entry) in entries.iter().enumerate() {
                    if section(index) != Some(which) || !shown_at(index) {
                        continue;
                    }
                    // A section's first row sits 2px below its header.
                    if rendered.is_empty() {
                        list = list.child(div().h(px(2.)));
                    }
                    rendered.push(ids[index].clone());
                    self.shown_rows.push(ids[index].clone());
                    let recorder = self.bounds.recorder(ids[index].clone());
                    list = list.child(
                        div()
                            .relative()
                            .child(self.node_row(
                                index,
                                entry,
                                &ids[index],
                                active == index,
                                (0..entries.len()).any(|other| {
                                    entries[other].parent == Some(index) && kept[other]
                                }),
                                filtering,
                                {
                                    // Each ancestor's vertical guide continues through
                                    // this row while that ancestor has siblings still
                                    // to come.
                                    let mut continues = vec![false; entry.depth];
                                    let mut ancestor = entry.parent;
                                    while let Some(at) = ancestor {
                                        let depth = entries[at].depth;
                                        if depth < continues.len() {
                                            continues[depth] = has_next_sibling(at);
                                        }
                                        ancestor = entries[at].parent;
                                    }
                                    Branch {
                                        continues,
                                        last_child: !has_next_sibling(index),
                                    }
                                },
                                cx,
                            ))
                            .child(recorder),
                    );
                }
            }
            matches += rendered.len();
            self.rendered[which as usize] = rendered;
        }
        if filtering && matches == 0 {
            list = list.child(
                div()
                    .mt(px(6.))
                    .ml(px(theme::SIDEBAR_INSET))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(t("No matching folders")),
            );
        }
        let tree_filter = prefs.show_folders || prefs.show_smart_folders;
        let dialog = self.dialog_layer(window, cx);
        if self
            .smart_editor
            .as_ref()
            .is_some_and(|(editor, _)| editor.read(cx).finished())
        {
            self.smart_editor = None;
            window.focus(&self.home, cx);
        }
        let smart_editor = self.smart_editor.as_ref().map(|(editor, _)| editor.clone());
        // The sidebar's 1px right border is drawn over the app background,
        // and the rail colour fills the area inside the border. While the
        // main window shows the system's frosted material
        // (`ui::window_chrome::rails_translucent`), the sidebar has no
        // background of its own: the shell (`Shell::render`) draws the app
        // background under the border, and the area inside the border is
        // filled with the partly transparent rail colour
        // (`ui::window_chrome::rail_fill`).
        let translucent = ui::window_chrome::rails_translucent(cx);
        let rail_fill = ui::window_chrome::rail_fill(cx);
        div()
            .id("sidebar")
            .size_full()
            .when(!translucent, |sidebar| sidebar.bg(theme.background()))
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .size_full()
                    .bg(rail_fill)
                    .font_family(theme::FONT_INSPECTOR)
                    .flex()
                    .flex_col()
                    .child(self.library(window, cx))
                    .children(
                        self.library_popover
                            .read(cx)
                            .is_open()
                            .then(|| self.library_popover.clone()),
                    )
                    .child({
                        let scroller = div()
                            .child(list)
                            .child(self.bounds.list_recorder())
                            .child(ui::scrollbar("sidebar-scrollbar", &self.scroll, window, cx))
                            .children(ui::scroll_to_top(
                                "sidebar-return-to-top",
                                &self.scroll,
                                RETURN_TO_TOP_AFTER,
                                // The button's bottom edge is 48px above the
                                // bottom of the sidebar. While the tree
                                // filter is shown, 38px of that distance is
                                // the filter's footer below this list, and
                                // the remaining 10px is inside the list.
                                if tree_filter { 10. } else { 48. },
                                window,
                                cx,
                            ));
                        if translucent && tree_filter {
                            // The last 10px of the list are not shown (see the
                            // footer below). An opaque strip cannot hide them
                            // while the frosted material is shown, because the
                            // strip would stand out against the translucent
                            // rail. So the list is clipped 10px above the
                            // bottom of its area, and laid out in the whole
                            // area as before.
                            div().relative().flex_1().min_h_0().child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .bottom(px(10.))
                                    .overflow_hidden()
                                    .child(
                                        scroller
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .right_0()
                                            .bottom(px(-10.)),
                                    ),
                            )
                        } else {
                            scroller.relative().flex_1().min_h_0()
                        }
                    })
                    .children(tree_filter.then(|| {
                        div()
                            .relative()
                            .h(px(38.))
                            .px(px(10.))
                            .pb(px(10.))
                            .flex_none()
                            // The footer covers the last 10px of the list with
                            // a strip in the rail colour. While the frosted
                            // material is shown, the list is clipped there
                            // instead (see above).
                            .children((!translucent).then(|| {
                                div()
                                    .absolute()
                                    .top(px(-10.))
                                    .left_0()
                                    .right_0()
                                    .h(px(10.))
                                    .bg(theme.rail())
                            }))
                            .child(
                                div()
                                    .key_context("SidebarFilter")
                                    .on_action(cx.listener(
                                        |this, _: &keyboard::EnterRows, window, cx| {
                                            this.enter_rows(window, cx)
                                        },
                                    ))
                                    .child(
                                        ui::field(
                                            "sidebar-filter",
                                            &self.filter,
                                            Look::SidebarFilter,
                                        )
                                        .icon("toolbar-filter", 16., 8., theme.text())
                                        .clearable()
                                        .clear_name(t("Clear folder filter")),
                                    ),
                            )
                    })),
            )
            .children(dialog)
            .children(smart_editor)
            .child(self.folder_picker.clone())
            .child(self.auto_tag_picker.clone())
            .children(self.drag_capture(cx))
            .children(self.drag_ghost(window, cx))
    }
}

impl Sidebar {
    /// Builds the row of a folder or smart folder in the Quick Access
    /// section. The row shows the node's glyph, name and count. It is not
    /// indented and has no tree guides or disclosure triangle. A click
    /// selects and opens the node (`click_node`), and a right-button press
    /// opens the node's context menu (`node_menu`). The node's row in the
    /// Folders or Smart Folders section uses the same two functions.
    fn quick_row(
        &self,
        index: usize,
        entry: &Navigation,
        active: bool,
        cx: &mut Context<Self>,
    ) -> ui::sidebar_row::SidebarRow {
        let prefs = Preferences::current(cx);
        let id = entry.id();
        let icon = match &entry.icon {
            Some(name) if crate::assets::has_icon(name) => name.clone(),
            None if entry.group => "layout-grid".into(),
            _ => "IconFolder".into(),
        };
        let node = id.clone();
        ui::sidebar_row(
            ElementId::Name(format!("quick-{id}").into()),
            icon,
            entry.label.clone(),
        )
        .tint(entry.tint.map(rgb))
        .count(entry.count.filter(|_| prefs.show_counts))
        .active(active)
        .context_highlight(!active && self.context.as_deref() == Some(id.as_str()))
        .focused(self.keyboard_row.as_deref() == Some(format!("quick-{id}").as_str()))
        .dragging(self.grid_dragging || self.node_drag.is_some())
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            this.click_node(index, node.clone(), event.modifiers(), cx)
        }))
        .on_secondary(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.node_menu(index, event.position, window, cx)
            }),
        )
    }

    /// Builds the row of one folder or smart folder in the Folders or Smart
    /// Folders section. `has_children` says whether the node has a child
    /// that the filter keeps, `filtering` says whether the filter field has
    /// text, and `branch` says which tree guides the row draws.
    #[allow(clippy::too_many_arguments)]
    fn node_row(
        &self,
        index: usize,
        entry: &Navigation,
        id: &str,
        active: bool,
        has_children: bool,
        filtering: bool,
        branch: Branch,
        cx: &mut Context<Self>,
    ) -> ui::sidebar_row::SidebarRow {
        let prefs = Preferences::current(cx);
        let expanded = filtering || !self.collapsed.contains(id);
        let group = entry.group;
        let in_section = section_of(&entry.scope, false).is_some();
        // The glyph is the icon the user chose for the node. A node without
        // one shows the grid icon when it is a smart folder group, and
        // otherwise a folder icon, which is the open folder while the node
        // has children and its branch is expanded. When the icon picker no
        // longer has the name the user chose, the row shows the closed
        // folder icon.
        let icon = match &entry.icon {
            Some(name) if !in_section || crate::assets::has_icon(name) => name.as_str(),
            Some(_) => "IconFolder",
            None if group => "layout-grid",
            None if has_children && expanded => "IconFolderOpen",
            None => "IconFolder",
        }
        .to_owned();
        let node = id.to_owned();
        let double_click = prefs.double_click;
        // The row's element id is the node's id, so the row keeps its state
        // (its highlight and disclosure animations) when the list is rebuilt.
        let mut row = ui::sidebar_row(
            ElementId::Name(id.to_owned().into()),
            icon,
            entry.label.clone(),
        )
        .depth(entry.depth)
        .tint(entry.tint.map(rgb))
        .count(entry.count.filter(|_| prefs.show_counts))
        .guides(prefs.show_guides)
        .active(active)
        .selected(!active && self.selection.contains(id))
        .context_highlight(!active && self.context.as_deref() == Some(id))
        .focused(self.keyboard_row.as_deref() == Some(id))
        .drop_highlight(self.grid_drop.as_deref() == Some(id))
        .dragging(self.grid_dragging || self.node_drag.is_some())
        .drop(
            self.drop
                .as_ref()
                .filter(|(target, _)| target == id)
                .map(|(_, position)| *position),
        )
        .rename(
            self.renaming
                .as_ref()
                .filter(|open| open.node == id)
                .map(|open| open.input.clone()),
        )
        .disclosure(has_children.then_some(expanded))
        .branch(branch)
        .on_press({
            let node = id.to_owned();
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.press_node(node.clone(), event.position, cx)
            })
        })
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            // When this click is the release of the mouse button that ended
            // a drag of a row, it opens nothing.
            if std::mem::take(&mut this.swallow_click) {
                return;
            }
            // A smart folder group cannot be opened. A click on it collapses
            // or expands its branch instead.
            if group {
                if has_children && !filtering {
                    this.toggle_collapsed(&node, cx);
                }
            } else {
                this.click_node(index, node.clone(), event.modifiers(), cx);
            }
            // The second click of a double-click also does what the
            // double-click setting says: it starts renaming the row, or it
            // collapses or expands the row's branch.
            if event.click_count() == 2 {
                match double_click {
                    DoubleClick::Rename => {
                        let name = this.label_of(&node, cx).unwrap_or_default();
                        this.start_rename(node.clone(), name, window, cx);
                    }
                    DoubleClick::Collapse if has_children && !filtering => {
                        this.toggle_collapsed(&node, cx)
                    }
                    DoubleClick::Collapse => {}
                }
            }
        }))
        .on_secondary(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.node_menu(index, event.position, window, cx)
            }),
        );
        // While the filter field has text, every branch is drawn expanded,
        // so the row's disclosure triangle does nothing.
        if !filtering {
            let node = id.to_owned();
            row =
                row.on_toggle(cx.listener(move |this, _, _, cx| this.toggle_collapsed(&node, cx)));
        }
        row
    }
}
