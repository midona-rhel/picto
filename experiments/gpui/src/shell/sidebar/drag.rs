//! Drags in the sidebar: a folder or smart-folder row dragged within its
//! tree to reorder or nest it (`Sidebar.tsx`'s folder drag), and items
//! dragged from the grid onto All, Inbox, Trash or a folder
//! (`dragState.ts`, `dragController.executeDrop`).
use super::{Sidebar, folder_id, smart_folder_id, tree};
use crate::grid::drag::{self, DragEvent, ItemDrag};
use crate::session::Reread;
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{FolderId, Lifecycle, SmartFolderId};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Where the rows that take drops were last painted, by node id, and the
/// list's visible area, so the pointer can be matched to a row.
#[derive(Clone, Default)]
pub struct RowBounds {
    rows: Rc<RefCell<Vec<(String, Bounds<Pixels>)>>>,
    list: Rc<Cell<Bounds<Pixels>>>,
}

impl RowBounds {
    /// Forgets the rows before the list is drawn again.
    pub fn clear(&self) {
        self.rows.borrow_mut().clear();
    }

    /// An element over a row that records where the row is.
    pub fn recorder(&self, id: String) -> impl IntoElement {
        let rows = self.rows.clone();
        canvas(
            move |bounds, _, _| rows.borrow_mut().push((id, bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    /// An element over the list that records its visible area.
    pub fn list_recorder(&self) -> impl IntoElement {
        let list = self.list.clone();
        canvas(move |bounds, _, _| list.set(bounds), |_, _, _, _| {})
            .absolute()
            .inset_0()
    }

    /// Where a row was last drawn, and the list's visible area.
    pub fn row(&self, id: &str) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        self.rows
            .borrow()
            .iter()
            .find(|(row, _)| row == id)
            .map(|(_, bounds)| (*bounds, self.list.get()))
    }

    /// The row under the pointer, among those `accepts`, and how far down
    /// the row the pointer is, from 0 to 1. Rows scrolled out of the list's
    /// view take nothing.
    pub fn hit(
        &self,
        position: Point<Pixels>,
        accepts: impl Fn(&str) -> bool,
    ) -> Option<(String, f32)> {
        if !self.list.get().contains(&position) {
            return None;
        }
        self.rows
            .borrow()
            .iter()
            .find(|(id, bounds)| bounds.contains(&position) && accepts(id))
            .map(|(id, bounds)| {
                let ratio = f32::from(position.y - bounds.top()) / f32::from(bounds.size.height);
                (id.clone(), ratio)
            })
    }
}

/// A folder or smart-folder row being dragged.
pub struct NodeDrag {
    pub node: String,
    pub position: Point<Pixels>,
    label: String,
    icon: String,
    tint: Option<Rgba>,
}

/// What a grid drop onto a row does: the lifecycle a system row stands for,
/// or the folder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridDrop {
    Lifecycle(Lifecycle),
    Folder(FolderId),
}

/// The rows that take items from the grid: All, Inbox and Trash by the
/// lifecycle they show, and every folder.
pub fn grid_drop(node: &str) -> Option<GridDrop> {
    match node {
        "system:active" => Some(GridDrop::Lifecycle(Lifecycle::Active)),
        "system:inbox" => Some(GridDrop::Lifecycle(Lifecycle::Inbox)),
        "system:trash" => Some(GridDrop::Lifecycle(Lifecycle::Trash)),
        _ => folder_id(node).map(GridDrop::Folder),
    }
}

impl Sidebar {
    /// Follows the grid's drag: the row under the pointer that takes items
    /// shows the drop look, and takes them when they are dropped.
    pub(super) fn grid_drag(
        &mut self,
        event: &DragEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DragEvent::Moved => {
                let position = drag::model(cx)
                    .read(cx)
                    .items
                    .as_ref()
                    .map(|items| items.position);
                let target = position.and_then(|position| {
                    self.bounds
                        .hit(position, |id| grid_drop(id).is_some())
                        .map(|(id, _)| id)
                });
                if self.grid_drop != target || !self.grid_dragging {
                    self.grid_drop = target;
                    self.grid_dragging = true;
                    cx.notify();
                }
            }
            DragEvent::Dropped(items) => {
                self.grid_dragging = false;
                let target = self
                    .bounds
                    .hit(items.position, |id| grid_drop(id).is_some())
                    .and_then(|(id, _)| grid_drop(&id));
                self.grid_drop = None;
                cx.notify();
                if let Some(target) = target {
                    self.drop_items(items.clone(), target, window, cx);
                }
            }
        }
    }

    /// `dragController.executeDrop`: a folder adds the items, and makes
    /// them active when they came from the Inbox or the Trash; All, Inbox
    /// and Trash set their lifecycle.
    fn drop_items(
        &mut self,
        items: ItemDrag,
        target: GridDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.session.read(cx).target_for(items.roots.clone());
        let activate = matches!(items.source, ItemScope::Inbox | ItemScope::Trash);
        self.edit_reading(
            move |library| {
                match target {
                    GridDrop::Folder(folder) => {
                        library.add_to_folder(&selection, folder)?;
                        if activate {
                            library.set_lifecycle(&selection, Lifecycle::Active)?;
                        }
                    }
                    GridDrop::Lifecycle(lifecycle) => {
                        library.set_lifecycle(&selection, lifecycle)?;
                    }
                }
                Ok(())
            },
            Reread::RESULTS,
            |_, (), _, _| {},
            window,
            cx,
        );
    }

    /// A left press on a folder or smart-folder row, which becomes a drag
    /// once the pointer moves 5px.
    pub(super) fn press_node(
        &mut self,
        node: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.swallow_click = false;
        if self.renaming.as_ref().is_some_and(|open| open.node == node) {
            return;
        }
        self.press = Some((node, position));
        cx.notify();
    }

    /// The pointer moved with the button held after a press on a row.
    fn node_drag_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.node_drag.is_none() {
            let Some((node, from)) = self.press.clone() else {
                return;
            };
            if !drag::past_threshold(from, position) {
                return;
            }
            let entry = {
                let nav = &self.session.read(cx).nav;
                nav.iter().find(|entry| entry.id() == node).cloned()
            };
            let Some(entry) = entry else {
                self.press = None;
                return;
            };
            let smart = node.starts_with("smart:");
            let label = if self.selection.contains(&node) && self.selection.len() > 1 {
                format!("{} items", self.selection.len())
            } else {
                entry.label.clone()
            };
            self.node_drag = Some(NodeDrag {
                node,
                position,
                label,
                icon: match entry.icon {
                    Some(icon) if crate::assets::has_icon(&icon) => icon,
                    _ if smart => "bookmark".into(),
                    _ => "folder".into(),
                },
                tint: entry.tint.map(rgb),
            });
        }
        let Some(dragged) = self.node_drag.as_mut() else {
            return;
        };
        dragged.position = position;
        let dragged = dragged.node.clone();
        let prefix = if dragged.starts_with("smart:") {
            "smart:"
        } else {
            "folder:"
        };
        let nodes = tree::nodes(&self.session.read(cx).nav, prefix);
        self.drop = self
            .bounds
            .hit(position, |id| {
                id.starts_with(prefix) && tree::can_take(&nodes, &dragged, id)
            })
            .map(|(id, ratio)| (id, tree::position_at(ratio)));
        cx.notify();
    }

    /// The button came up: a drag over a row moves the dragged node, or the
    /// selection it is part of, as `planSidebarTreeDrop` plans it.
    fn node_drag_released(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.press = None;
        let Some(dragged) = self.node_drag.take() else {
            return;
        };
        self.swallow_click = true;
        let target = self.drop.take();
        cx.notify();
        let Some((target, position)) = target else {
            return;
        };
        let prefix = if dragged.node.starts_with("smart:") {
            "smart:"
        } else {
            "folder:"
        };
        let nodes = tree::nodes(&self.session.read(cx).nav, prefix);
        let selected = self.selection.iter().cloned().collect::<Vec<_>>();
        let Some(plan) = tree::plan_drop(&nodes, &selected, &dragged.node, &target, position)
        else {
            return;
        };
        if prefix == "folder:" {
            let moving = plan
                .moving
                .iter()
                .filter_map(|id| folder_id(id))
                .collect::<Vec<_>>();
            let parent = plan.parent.as_deref().and_then(folder_id);
            let ordered = plan
                .ordered
                .iter()
                .filter_map(|id| folder_id(id))
                .collect::<Vec<_>>();
            self.edit(
                move |library| move_folders(library, &moving, parent, &ordered),
                |_, (), _, _| {},
                window,
                cx,
            );
        } else {
            let moving = plan
                .moving
                .iter()
                .filter_map(|id| smart_folder_id(id))
                .collect::<Vec<_>>();
            let parent = plan.parent.as_deref().and_then(smart_folder_id);
            let ordered = plan
                .ordered
                .iter()
                .filter_map(|id| smart_folder_id(id))
                .collect::<Vec<_>>();
            self.edit(
                move |library| move_smart_folders(library, &moving, parent, &ordered),
                |_, (), _, _| {},
                window,
                cx,
            );
        }
    }

    /// While a row is pressed or dragged, the pointer is followed anywhere
    /// in the window and the release heard there, as pointer capture does.
    pub(super) fn drag_capture(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.press.is_none() && self.node_drag.is_none() {
            return None;
        }
        let entity = cx.entity().downgrade();
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let _ = moved.update(cx, |this, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                this.node_drag_moved(event.position, cx);
                            } else {
                                this.node_drag_released(window, cx);
                            }
                        });
                    });
                    let released = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                            let _ =
                                released.update(cx, |this, cx| this.node_drag_released(window, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// The drag ghost: a highlighted row 200px wide under the pointer, with
    /// the grabbing cursor over the whole window.
    pub(super) fn drag_ghost(&self, window: &Window, cx: &App) -> Option<AnyElement> {
        let dragged = self.node_drag.as_ref()?;
        let theme = theme::theme(cx);
        let viewport = window.viewport_size();
        let ghost = div()
            .absolute()
            .left(dragged.position.x - px(20.))
            .top(dragged.position.y - px(13.))
            .w(px(200.))
            .h(px(theme::SIDEBAR_ROW))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .rounded(px(5.))
            .bg(theme.surface_active())
            .border_1()
            .border_color(theme.border())
            .text_color(theme.text())
            .font_family(theme::FONT_INSPECTOR)
            .text(theme::TEXT_MD)
            .overflow_hidden()
            .opacity(0.9)
            .shadow(vec![
                BoxShadow::new(px(0.), px(2.), rgba(theme::BLACK_20).into()).blur_radius(px(8.)),
            ])
            .child(
                crate::ui::icon(
                    dragged.icon.clone(),
                    theme::SIDEBAR_ICON,
                    dragged.tint.unwrap_or(theme.text_tertiary()),
                )
                .flex_none(),
            )
            .child(div().min_w_0().truncate().child(dragged.label.clone()));
        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("sidebar-drag")
                        .w(viewport.width)
                        .h(viewport.height)
                        .cursor(CursorStyle::ClosedHand)
                        .child(ghost),
                ),
            )
            .with_priority(30)
            .into_any_element(),
        )
    }
}

/// `foldersController.moveMany`: each folder under the new parent, then the
/// parent's children in the planned order.
fn move_folders(
    library: &picto_library::Library,
    moving: &[FolderId],
    parent: Option<FolderId>,
    ordered: &[FolderId],
) -> picto_library::Result<()> {
    for id in moving {
        library.move_folder(*id, parent)?;
    }
    if !ordered.is_empty() {
        library.reorder_folder_children(parent, ordered)?;
    }
    Ok(())
}

/// `smartFoldersController.moveMany`.
fn move_smart_folders(
    library: &picto_library::Library,
    moving: &[SmartFolderId],
    parent: Option<SmartFolderId>,
    ordered: &[SmartFolderId],
) -> picto_library::Result<()> {
    for id in moving {
        let mut input = super::smart_input(library, *id)?;
        input.parent_id = parent;
        library.update_smart_folder(*id, input)?;
    }
    if !ordered.is_empty() {
        library.reorder_smart_folder_children(parent, ordered)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GridDrop, grid_drop};
    use picto_library::{FolderId, Lifecycle};

    #[test]
    fn all_inbox_trash_and_folders_take_items_from_the_grid() {
        assert_eq!(
            grid_drop("system:inbox"),
            Some(GridDrop::Lifecycle(Lifecycle::Inbox))
        );
        assert_eq!(
            grid_drop("system:active"),
            Some(GridDrop::Lifecycle(Lifecycle::Active))
        );
        assert_eq!(grid_drop("folder:4"), Some(GridDrop::Folder(FolderId(4))));
        assert_eq!(grid_drop("smart:4"), None);
        assert_eq!(grid_drop("system:untagged"), None);
    }
}
