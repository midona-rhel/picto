//! This file handles the two kinds of drag in the sidebar. The row of a
//! folder or smart folder can be dragged within its tree, which moves the
//! node before, after or inside another node. Items dragged from the grid
//! can be dropped on the rows All, Inbox and Trash and on the row of a
//! folder.
use super::{Sidebar, folder_id, smart_folder_id, tree};
use crate::grid::drag::{self, DragEvent, ItemDrag};
use crate::i18n::t;
use crate::session::{Report, Reread, edit_then_in};
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{FolderId, Lifecycle, SmartFolderId};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Holds the bounds at which each row was last painted, by row id, and the
/// bounds of the visible part of the list. They are used to find the row
/// that is under the pointer.
#[derive(Clone, Default)]
pub struct RowBounds {
    rows: Rc<RefCell<Vec<(String, Bounds<Pixels>)>>>,
    list: Rc<Cell<Bounds<Pixels>>>,
}

impl RowBounds {
    /// Removes the recorded bounds of all rows. This is called before the
    /// list is drawn again.
    pub fn clear(&self) {
        self.rows.borrow_mut().clear();
    }

    /// Returns an element that covers a row and records the row's bounds
    /// under `id` when it is laid out.
    pub fn recorder(&self, id: String) -> impl IntoElement {
        let rows = self.rows.clone();
        canvas(
            move |bounds, _, _| rows.borrow_mut().push((id, bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    /// Returns an element that covers the list and records the bounds of
    /// the visible part of the list when it is laid out.
    pub fn list_recorder(&self) -> impl IntoElement {
        let list = self.list.clone();
        canvas(move |bounds, _, _| list.set(bounds), |_, _, _, _| {})
            .absolute()
            .inset_0()
    }

    /// Returns the bounds at which the row `id` was last drawn, and the
    /// bounds of the visible part of the list.
    pub fn row(&self, id: &str) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        self.rows
            .borrow()
            .iter()
            .find(|(row, _)| row == id)
            .map(|(_, bounds)| (*bounds, self.list.get()))
    }

    /// Returns the row under the pointer for which `accepts` returns true.
    /// With the row it returns how far down the row the pointer is, from 0
    /// at the row's top edge to 1 at its bottom edge. Returns `None` when
    /// the pointer is outside the visible part of the list, so a row that is
    /// scrolled out of view is never returned.
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

/// Holds the row of a folder or smart folder that is being dragged: the
/// node's id, the pointer's position, and the label, icon and colour that
/// the picture of the row under the pointer shows.
pub struct NodeDrag {
    pub node: String,
    pub position: Point<Pixels>,
    label: String,
    icon: String,
    tint: Option<Rgba>,
}

/// What a drop of items from the grid on a row does. A drop on All, Inbox
/// or Trash gives the items that lifecycle, and a drop on a folder adds the
/// items to the folder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridDrop {
    Lifecycle(Lifecycle),
    Folder(FolderId),
}

/// Returns what a drop of items from the grid on the row `node` does, or
/// `None` when the row does not accept items. The rows All, Inbox and Trash
/// and the row of every folder accept items.
pub fn grid_drop(node: &str) -> Option<GridDrop> {
    match node {
        "system:active" => Some(GridDrop::Lifecycle(Lifecycle::Active)),
        "system:inbox" => Some(GridDrop::Lifecycle(Lifecycle::Inbox)),
        "system:trash" => Some(GridDrop::Lifecycle(Lifecycle::Trash)),
        _ => folder_id(node).map(GridDrop::Folder),
    }
}

/// Returns the function that changes the library for items dropped on
/// `target`. A drop on a folder adds the items to the folder, and also
/// makes them active when they were dragged from the Inbox or the Trash. A
/// drop on All, Inbox or Trash gives the items that lifecycle.
fn drop_work(
    items: &ItemDrag,
    target: GridDrop,
) -> impl FnOnce(&picto_library::Library) -> picto_library::Result<()> + Send + 'static {
    let selection = items.target.clone();
    let activate = matches!(items.source, ItemScope::Inbox | ItemScope::Trash);
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
    }
}

/// Adds the dropped `items` to `folder`, as a drop on the folder's row in
/// the sidebar does. The grid calls this when items are dropped on a
/// subfolder tile.
pub fn drop_into_folder(
    session: &Entity<crate::session::Session>,
    items: ItemDrag,
    folder: FolderId,
    cx: &mut App,
) {
    let work = drop_work(&items, GridDrop::Folder(folder));
    session.update(cx, |session, cx| {
        let _ = session.edit(work, Reread::RESULTS, cx);
    });
}

impl Sidebar {
    /// Handles an event of a drag of items from the grid. While the pointer
    /// moves, the row under it that accepts items is drawn highlighted. When
    /// the items are dropped on such a row, the drop is carried out.
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
            DragEvent::Cancelled => {
                self.grid_dragging = false;
                self.grid_drop = None;
                cx.notify();
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

    /// Carries out a drop of `items` from the grid on `target` in the
    /// library (`drop_work`), then reads the grid's results again.
    fn drop_items(
        &mut self,
        items: ItemDrag,
        target: GridDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let work = drop_work(&items, target);
        edit_then_in(
            &self.session,
            Report::default(),
            Reread::RESULTS,
            work,
            window,
            cx,
            |_, _, _, _| {},
        );
    }

    /// Records a press of the left mouse button on the row of a folder or
    /// smart folder. The press becomes a drag once the pointer has moved 5px
    /// from where it was pressed. A row that is being renamed is not
    /// dragged.
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

    /// Handles a move of the pointer while the left button is held after a
    /// press on a row. It starts the drag once the pointer has moved 5px,
    /// and then keeps `drop` set to the row under the pointer on which the
    /// dragged node can be dropped.
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
                    _ if smart => "IconBookmark".into(),
                    _ => "IconFolder".into(),
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

    /// Handles the release of the left button. When a drag ends over a row
    /// on which the dragged node can be dropped, the node is moved there in
    /// the library. When the dragged node is one of several selected nodes,
    /// all of them are moved (`tree::plan_drop`).
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
            edit_then_in(
                &self.session,
                Report::failure(t("Could not move folder")),
                Reread::NOTHING,
                move |library| move_folders(library, &moving, parent, &ordered),
                window,
                cx,
                |_, _, _, _| {},
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
            edit_then_in(
                &self.session,
                Report::failure(t("Could not move smart folder")),
                Reread::NOTHING,
                move |library| move_smart_folders(library, &moving, parent, &ordered),
                window,
                cx,
                |_, _, _, _| {},
            );
        }
    }

    /// Returns an element that, while a row is pressed or dragged, receives
    /// every move of the pointer and the release of the left button
    /// anywhere in the window. Returns `None` at other times.
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

    /// Draws the picture of the dragged row that follows the pointer: a
    /// highlighted row 200px wide with the node's icon and label. While it
    /// is shown, the pointer is the closed hand over the whole window.
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
            .with_priority(crate::ui::layer::DRAG)
            .into_any_element(),
        )
    }
}

/// Moves each of the folders `moving` under `parent`, then puts the
/// children of `parent` in the order `ordered`.
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

/// Moves each of the smart folders `moving` under `parent`, then puts the
/// children of `parent` in the order `ordered`.
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
