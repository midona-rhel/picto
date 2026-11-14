//! The sidebar's drawing: the library switcher, the system rows, Quick
//! Access, the Folders and Smart Folders sections and their rows.
use super::{
    DoubleClick, Preferences, QUICK_ACCESS, RETURN_TO_TOP_AFTER, SECTIONS, Section, Sidebar,
    Switcher, keyboard, section_of,
};
use crate::backend::{Navigation, Screen};
use crate::theme::Text as _;
use crate::ui::{self, field::Look, sidebar_row::Branch};
use crate::{motion, theme};
use gpui::*;

impl Sidebar {
    /// The library switcher (`LibrarySwitcherButton`): the cover, the
    /// library's name and a selector glyph, over a highlight shaped as the
    /// cover's circle joined to a bar. A click opens the library popover;
    /// while it is open the highlight takes the active surface and the
    /// glyph stays lit.
    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (cover, name) = {
            let session = self.session.read(cx);
            let name = session
                .library_name()
                .unwrap_or_else(|| "No library".into());
            (session.library_cover.clone(), name)
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
        // The cover stays decoded however far the grid scrolls.
        let pinned = cover.clone().map(|path| Resource::Path(path.into()));
        self.thumbnails
            .update(cx, |thumbnails, _| thumbnails.pin("library-cover", pinned));
        // A canvas takes no opacity of its own, so the fade is in the colour.
        let fill = theme::blend(
            gpui::transparent_black().into(),
            theme::blend(theme.surface_hover(), theme.surface_active(), active),
            lit,
        );
        let bounds = self.switcher.clone();
        let highlight = canvas(
            // The popover opens under the button's box, which this fills.
            move |button, _, _| bounds.set(button),
            move |bounds, _, window, _| {
                // A 24px-radius circle about the cover, and a 30px bar from
                // the circle's centre to the right edge, drawn as one outline
                // so the overlap is not painted twice.
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
        div()
            .id("library-switcher")
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
            .children(hint.read(cx).layer(&ui::tip("Switch Library"), 48., cx))
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs = Preferences::current(cx);
        crate::stats::rendered(crate::stats::View::Sidebar);
        // The grid's drags are followed from the first frame on, and the
        // folder picker's Move.
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
        // The focus look goes when the rows lose the keyboard.
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
        // The tree filter keeps each matching folder and its ancestors, and
        // opens every branch (`treeFilter.ts`). System rows are not filtered.
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
        // Whether a row has a sibling below it among the rows that show.
        let has_next_sibling = |index: usize| {
            (index + 1..entries.len()).any(|other| {
                entries[other].parent == entries[index].parent
                    && section(other) == section(index)
                    && kept[other]
            })
        };

        let mut list = div()
            .id("navigation")
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

        // The Subscriptions row's button while subscriptions run or are
        // held: it holds or releases every subscription.
        let activity = self.session.read(cx).subscriptions;
        // System rows, those the visibility menu leaves on.
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
                            "Resume all subscriptions".into()
                        } else {
                            "Pause all subscriptions".into()
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

        // Quick Access, the folders and smart folders pinned to it, in
        // their order, when there are any.
        let quick = self
            .quick_access(cx)
            .iter()
            .filter_map(|id| ids.iter().position(|known| known == id))
            .filter(|index| section(*index).is_some())
            .collect::<Vec<_>>();
        if prefs.show_quick_access && !quick.is_empty() {
            let expanded = !self.collapsed.contains(QUICK_ACCESS);
            list = list.child(
                ui::section_header("quick-access", "Quick Access")
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
                    ui::tip("New Folder").shortcut("file.newFolder", cx),
                    "folders",
                ),
                Section::SmartFolders => (ui::tip("New Smart Folder or Group"), "smart-folders"),
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
                                // A menu below the button, at its left edge.
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
                    .child("No matching folders"),
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
        // The rail's edge is drawn over the app background, as the shell's
        // border is in the Electron layout, with the rail surface inside it.
        div()
            .id("sidebar")
            .size_full()
            .bg(theme.background())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .size_full()
                    .bg(theme.rail())
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
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(list)
                            .child(self.bounds.list_recorder())
                            .child(ui::scrollbar("sidebar-scrollbar", &self.scroll, window, cx))
                            .children(ui::scroll_to_top(
                                "sidebar-return-to-top",
                                &self.scroll,
                                RETURN_TO_TOP_AFTER,
                                // 48px above the sidebar's foot, 38 of which
                                // is the filter below this list.
                                if tree_filter { 10. } else { 48. },
                                window,
                                cx,
                            )),
                    )
                    .children(tree_filter.then(|| {
                        div()
                            .relative()
                            .h(px(38.))
                            .px(px(10.))
                            .pb(px(10.))
                            .flex_none()
                            // The footer covers the last 10px of the list.
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-10.))
                                    .left_0()
                                    .right_0()
                                    .h(px(10.))
                                    .bg(theme.rail()),
                            )
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
                                        .clearable(),
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
    /// A Quick Access row: the node's glyph, name and count, without its
    /// tree, opening it and offering its menu as the tree's row does.
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
            _ => "folder".into(),
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

    /// One folder or smart-folder row.
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
        // `NodeIcon`: the glyph the user chose, else the group's grid, else
        // a folder that opens with its branch. A name the icon picker no
        // longer has falls back to the folder.
        let icon = match &entry.icon {
            Some(name) if !in_section || crate::assets::has_icon(name) => name.as_str(),
            Some(_) => "folder",
            None if group => "layout-grid",
            None if has_children && expanded => "folder-open",
            None => "folder",
        }
        .to_owned();
        let node = id.to_owned();
        let double_click = prefs.double_click;
        // Keyed by id, so a row keeps its state when the list is rebuilt.
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
            // The release that ends a drag opens nothing.
            if std::mem::take(&mut this.swallow_click) {
                return;
            }
            // A group is not a destination: a click folds or opens it.
            if group {
                if has_children && !filtering {
                    this.toggle_collapsed(&node, cx);
                }
            } else {
                this.click_node(index, node.clone(), event.modifiers(), cx);
            }
            // The second press of a double-click also does what the
            // setting says: rename, or fold the branch.
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
        // The filter holds every branch open.
        if !filtering {
            let node = id.to_owned();
            row =
                row.on_toggle(cx.listener(move |this, _, _, cx| this.toggle_collapsed(&node, cx)));
        }
        row
    }
}
