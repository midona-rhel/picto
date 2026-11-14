//! The folder picker's drawing: the tree's guide lines, the folder rows and
//! the Library row, the new-folder dialog and the picker itself.
use super::{FolderPicker, HEIGHT, ROW, Row, View, WIDTH};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    action_button::Look as ButtonLook,
    check::{self, Mark},
    field::Look,
    modal::Size,
    overlay_shell::{self},
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::predicate::SetMatchMode;

/// `FolderTree`'s guide lines: a line down through the rows of each
/// ancestor that continues, and this row's branch, which ends in a curve on
/// the last child.
fn guides(depth: usize, continues: Vec<bool>, last: bool, color: Rgba) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let unit = bounds.size.height / 26.;
            let top = bounds.top();
            let line = |window: &mut Window, x: Pixels, from: Pixels, to: Pixels| {
                window.paint_quad(fill(
                    Bounds::from_corners(point(x, from), point(x + px(1.), to)),
                    color,
                ));
            };
            if depth > 1 {
                for (index, continues) in continues.iter().enumerate() {
                    if *continues {
                        let x = bounds.left() + px(14. + index as f32 * 24.);
                        line(window, x, top, bounds.bottom());
                    }
                }
            }
            if depth == 0 {
                return;
            }
            let x = bounds.left() + px(14. + (depth as f32 - 1.) * 24.);
            let arm_y = (top + unit * 13.).floor();
            if last {
                // `M0 0 V9.5 A3.5 3.5 0 0 0 3.5 13 H13`
                let mut path = PathBuilder::stroke(px(1.));
                path.move_to(point(x + px(0.5), top));
                path.line_to(point(x + px(0.5), top + unit * 9.5));
                path.arc_to(
                    point(px(3.5), unit * 3.5),
                    px(0.),
                    false,
                    false,
                    point(x + px(4.), top + unit * 13.),
                );
                path.line_to(point(x + px(13.5), top + unit * 13.));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            } else {
                // `M0 0 V26 M0 13 H13`
                line(window, x, top, bounds.bottom());
                window.paint_quad(fill(
                    Bounds::from_corners(point(x, arm_y), point(x + px(13.), arm_y + px(1.))),
                    color,
                ));
            }
        },
    )
    .absolute()
    .inset_0()
}

impl FolderPicker {
    fn row(&self, index: usize, row: &Row, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let folder = row.folder.clone();
        let selected = self.selected.contains(&folder.id);
        let excluded = self
            .filter
            .as_ref()
            .is_some_and(|filter| filter.excluded.contains(&folder.id));
        let focused = self.focus_index == Some(index + self.key_offset());
        let indent = 4. + row.depth as f32 * 24.;
        let group = SharedString::from(format!("folder-row-{index}"));
        // `.row::before`: the highlight, kept clear of the tree's indent.
        let (fill, outline) = if selected {
            (theme.surface_active(), Some(theme.contrast(25.)))
        } else if focused {
            (theme.surface_hover(), Some(theme.border_focus()))
        } else {
            (gpui::transparent_black().into(), None)
        };
        let mut highlight_box = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .left(px(indent))
            .rounded(px(theme::RADIUS_MD))
            .bg(fill);
        if let Some(outline) = outline {
            highlight_box = highlight_box.border_1().border_color(outline);
        } else {
            highlight_box = highlight_box
                .group_hover(group.clone(), move |style| style.bg(theme.surface_hover()));
        }
        let name = overlay_shell::search_match(folder.name.clone(), &self.query, cx);
        let id = folder.id;
        let mut right = div()
            .flex_1()
            .min_w(px(26.))
            .max_w(relative(0.4))
            .ml_auto()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.));
        if let Some(parent) = &row.parent_name {
            right = right.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_secondary())
                    .group_hover(group.clone(), move |style| style.text_color(theme.text()))
                    .child(parent.clone()),
            );
        }
        right = if row.children {
            let expanded = row.expanded;
            right.child(
                div()
                    .id(("folder-expand", index))
                    .group(SharedString::from(format!("folder-expand-{index}")))
                    .size(px(26.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_active()))
                    .child(
                        svg()
                            .path("picker-chevron")
                            .size(px(12.))
                            .text_color(theme.text_secondary())
                            .group_hover(format!("folder-expand-{index}"), move |style| {
                                style.text_color(theme.text())
                            })
                            .with_transformation(Transformation::rotate(radians(if expanded {
                                std::f32::consts::FRAC_PI_2
                            } else {
                                0.
                            }))),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.folded.remove(&id) {
                            this.folded.insert(id);
                        }
                        cx.notify();
                    })),
            )
        } else {
            right.child(div().size(px(26.)).flex_none())
        };
        let menu_folder = folder.clone();
        div()
            .id(("folder-row", index))
            .group(group)
            .relative()
            .h(px(ROW))
            .flex_none()
            .pl(px(indent))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .text_color(theme.text())
            .whitespace_nowrap()
            .child(guides(
                row.depth,
                row.continues.clone(),
                row.last,
                theme.tree_guide(),
            ))
            .child(highlight_box)
            .child(
                div()
                    .relative()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(check::check_box(
                        match (excluded, selected, self.filter.is_some()) {
                            (true, _, _) => Mark::Excluded,
                            (false, true, true) => Mark::Included,
                            (false, true, false) => Mark::Selected,
                            (false, false, _) => Mark::Empty,
                        },
                        check::Size::Compact,
                    )),
            )
            .child(
                ui::icon("picker-folder", 20., folder.color.map_or(theme.text(), rgb))
                    .relative()
                    .flex_none(),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // `.rowExcluded .name`
                    .when(excluded, |name| name.opacity(0.7))
                    .child(name),
            )
            .child(right.relative())
            // `.rowExcluded`
            .when(excluded, |row| row.opacity(0.58))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.focus_index = Some(index);
                this.click(id, event.modifiers(), cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if this.filter.is_some() {
                        this.exclude(menu_folder.id, cx);
                    } else {
                        this.open_menu(menu_folder.clone(), event.position, window, cx);
                    }
                }),
            )
    }

    /// Move mode's Library row (`.checkRow`): the top level.
    fn root_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let chosen = self.parent_mode.as_ref().is_some_and(|mode| mode.root);
        let focused = self.focus_index == Some(0);
        let mut row = div()
            .id("folder-picker-library")
            .h(px(theme::CONTROL_MD))
            .flex_none()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .text_color(theme.text())
            .child(check::check_box(
                if chosen { Mark::Checked } else { Mark::Empty },
                check::Size::Compact,
            ))
            .child(ui::icon("picker-folder", 14., theme.text()).flex_none())
            .child(div().flex_1().min_w_0().truncate().child("Library"))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(mode) = &mut this.parent_mode {
                    mode.root = true;
                }
                this.selected.clear();
                cx.notify();
            }));
        row = if chosen {
            row.bg(theme.surface_active())
                .border_1()
                .border_color(theme.contrast(25.))
        } else if focused {
            row.border_1().border_color(theme.border_focus())
        } else {
            row.hover(move |style| style.bg(theme.subtle_overlay()))
        };
        row.into_any_element()
    }

    fn create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.create.layer(window, cx)?;
        let create = &dialog.content;
        let typed = !create.name.read(cx).value().trim().is_empty();
        let busy = create.busy;
        Some(
            ui::modal("folder-picker-create", create.title, dialog.presence)
                .size(Size::Small)
                .focus(&dialog.focus)
                .layer(crate::ui::layer::DIALOG + 1)
                .on_close({
                    let this = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.dismiss_create(cx));
                    }
                })
                .child(ui::field("folder-picker-name", &create.name, Look::Modal))
                .footer(
                    ui::action_button("folder-picker-cancel", ButtonLook::Standard)
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.create.close(cx);
                        })),
                )
                .footer(
                    ui::action_button("folder-picker-submit", ButtonLook::Primary)
                        .label("Create")
                        .disabled(!typed || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_create(cx))),
                )
                .into_any_element(),
        )
    }
}

impl Render for FolderPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu.layer(window, cx);
        let create = self.create_dialog(window, cx);
        if !self.is_open() {
            return div().into_any_element();
        }
        let theme = theme::theme(cx);
        let entity = cx.entity().downgrade();
        let (moved, nav, close, chosen) = (
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
            .child(ui::icon("picker-search", 14., theme.text_secondary()).flex_none())
            .child(div().flex_1().min_w_0().h_full().child(self.search.clone()));
        let tabs = overlay_shell::view_tabs(
            "folder-picker-view",
            &[
                ("picker-folders", "All folders"),
                ("picker-history", "Recent folders"),
                ("picker-checks", "Selected folders"),
            ],
            match self.view {
                View::All => 0,
                View::Recent => 1,
                View::Selected => 2,
            },
            move |index, _, cx| {
                let _ = chosen.update(cx, |this, cx| {
                    this.view = [View::All, View::Recent, View::Selected][index];
                    this.focus_index = None;
                    cx.notify();
                });
            },
        );

        let rows = self.visible(cx);
        let root = self.shows_root().then(|| self.root_row(cx));
        let body = if rows.is_empty() {
            div()
                .h(px(60.))
                .flex()
                .items_center()
                .justify_center()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child("No folders")
                .into_any_element()
        } else {
            let mut tree = div().w_full().p(px(4.)).flex().flex_col();
            for (index, row) in rows.iter().enumerate() {
                tree = tree.child(self.row(index, row, cx));
            }
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("folder-picker-tree")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .pr(px(ui::scrollbar::GUTTER))
                        .child(tree),
                )
                .child(ui::scrollbar(
                    "folder-picker-scrollbar",
                    &self.scroll,
                    window,
                    cx,
                ))
                .into_any_element()
        };

        let hints = div().flex().items_center().gap(px(12.)).min_w_0();
        let hints = if self.filter.is_some() {
            hints
                .child(
                    overlay_shell::hint(cx)
                        .child("Select")
                        .child(overlay_shell::kbd("L-Click", cx)),
                )
                .child(
                    overlay_shell::hint(cx)
                        .child("Exclude")
                        .child(overlay_shell::kbd("R-Click", cx)),
                )
        } else {
            hints
                .child(
                    overlay_shell::hint(cx)
                        .child("Switch ")
                        .child(overlay_shell::tab_key(cx)),
                )
                .child(
                    overlay_shell::hint(cx).child("Move ").child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(overlay_shell::kbd("↑", cx))
                            .child(overlay_shell::kbd("↓", cx)),
                    ),
                )
                .child(
                    overlay_shell::hint(cx)
                        .child("Select ")
                        .child(overlay_shell::kbd("↵", cx)),
                )
        };
        // The filter mode's match rule follows the view tabs.
        let logic = self.filter.as_ref().map(|filter| {
            let modes = [SetMatchMode::Any, SetMatchMode::All, SetMatchMode::Exact];
            let logic = cx.entity().downgrade();
            ui::segmented::segmented(
                "folder-picker-logic",
                ui::segmented::Look::Logic,
                vec![
                    ui::segmented::segment("logic-any", "Match any"),
                    ui::segmented::segment("logic-all", "Match all"),
                    ui::segmented::segment("logic-exact", "Match exactly"),
                ],
                modes.iter().position(|mode| *mode == filter.mode),
            )
            .on_select(move |index, _, cx| {
                let _ = logic.update(cx, |this, cx| this.set_mode(modes[index], cx));
            })
        });
        let mut shell = overlay_shell::overlay_shell(
            "folder-picker",
            self.geometry,
            WIDTH,
            HEIGHT,
            &self.focus,
            move |window, cx| {
                let _ = close.update(cx, |this, cx| this.close(window, cx));
            },
        )
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
        .header(search)
        .header(tabs);
        if let Some(logic) = logic {
            shell = shell.header(logic);
        }
        let shell = shell
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .children(root)
                    .child(body),
            )
            .footer(hints)
            .footer(if self.parent_mode.is_some() {
                div().ml_auto().flex_none().child(
                    ui::action_button("folder-picker-move", ButtonLook::Primary)
                        .label("Move")
                        .on_click(cx.listener(|this, _, window, cx| this.apply_parent(window, cx))),
                )
            } else {
                div().ml_auto().flex_none().child(
                    overlay_shell::hint(cx)
                        .child("Close ")
                        .child(overlay_shell::kbd("Esc", cx)),
                )
            });
        div()
            .child(shell)
            .children(create)
            .children(menu)
            .into_any_element()
    }
}
