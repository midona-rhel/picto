//! This file holds the rules for moving nodes in the tree of folders and in
//! the tree of smart folders: under which nodes a node can be moved, which
//! of several selected nodes are moved, and what a drop of a dragged node on
//! a row does.
use crate::backend::Navigation;
use crate::ui::sidebar_row::DropPosition;

/// One node of a tree: its id and the id of its parent. A list of nodes is
/// in the order in which the sidebar shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
}

/// Returns the nodes of one section out of the list of destinations.
/// `prefix` is `folder:` or `smart:`.
pub fn nodes(entries: &[Navigation], prefix: &str) -> Vec<Node> {
    let ids = entries.iter().map(Navigation::id).collect::<Vec<_>>();
    entries
        .iter()
        .enumerate()
        .filter(|(index, _)| ids[*index].starts_with(prefix))
        .map(|(index, entry)| Node {
            id: ids[index].clone(),
            parent: entry.parent.map(|parent| ids[parent].clone()),
        })
        .collect()
}

/// Returns whether `node` is a child of `ancestor` or a descendant further
/// down.
pub fn is_descendant(nodes: &[Node], node: &str, ancestor: &str) -> bool {
    let parent_of = |id: &str| {
        nodes
            .iter()
            .find(|candidate| candidate.id == id)
            .and_then(|candidate| candidate.parent.clone())
    };
    let mut seen = Vec::new();
    let mut at = parent_of(node);
    while let Some(parent) = at {
        if parent == ancestor {
            return true;
        }
        if seen.contains(&parent) {
            return false;
        }
        seen.push(parent.clone());
        at = parent_of(&parent);
    }
    false
}

/// Returns the ids in `ids` that have no ancestor in `ids`. A selected node
/// whose ancestor is also selected moves with that ancestor, so it is not
/// moved by itself.
pub fn top_level(nodes: &[Node], ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            !ids.iter()
                .any(|other| other != *id && is_descendant(nodes, id, other))
        })
        .cloned()
        .collect()
}

/// Returns every node under which the nodes `moving` can be moved, in the
/// order of `nodes`. These are the nodes that are neither moving nor under
/// a moving node.
pub fn move_targets(nodes: &[Node], moving: &[String]) -> Vec<String> {
    nodes
        .iter()
        .map(|node| node.id.clone())
        .filter(|candidate| {
            moving
                .iter()
                .all(|id| candidate != id && !is_descendant(nodes, candidate, id))
        })
        .collect()
}

/// Returns where a drop on a row lands. `ratio` is the pointer's distance
/// from the top of the row divided by the row's height. The top 30% of the
/// row drops before the row, the bottom 30% drops after it, and the middle
/// drops inside it.
pub fn position_at(ratio: f32) -> DropPosition {
    if ratio < 0.3 {
        DropPosition::Before
    } else if ratio > 0.7 {
        DropPosition::After
    } else {
        DropPosition::Inside
    }
}

/// Returns whether `dragged` can be dropped on `target`. A node cannot be
/// dropped on itself or on one of its descendants.
pub fn can_take(nodes: &[Node], dragged: &str, target: &str) -> bool {
    target != dragged && !is_descendant(nodes, target, dragged)
}

/// Holds what a drop does: the nodes that move, the parent they move under
/// (`None` for the top level), and the new order of that parent's children.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub moving: Vec<String>,
    pub parent: Option<String>,
    pub ordered: Vec<String>,
}

/// Returns what a drop of `dragged` on `target` at `position` does. The
/// dragged node moves alone, unless it is one of several selected nodes of
/// its section, in which case all of those move. A node that has an
/// ancestor among the moving nodes moves with that ancestor. Returns `None`
/// when the target is a moving node or is under one.
pub fn plan_drop(
    nodes: &[Node],
    selected: &[String],
    dragged: &str,
    target: &str,
    position: DropPosition,
) -> Option<Plan> {
    let target_node = nodes.iter().find(|node| node.id == target)?;
    let prefix = if dragged.starts_with("smart:") {
        "smart:"
    } else {
        "folder:"
    };
    let selected = selected
        .iter()
        .filter(|id| id.starts_with(prefix))
        .cloned()
        .collect::<Vec<_>>();
    let raw = if selected.iter().any(|id| id == dragged) && selected.len() > 1 {
        selected
    } else {
        vec![dragged.to_owned()]
    };
    let moving_set = top_level(nodes, &raw);
    if moving_set.iter().any(|id| id == target) {
        return None;
    }
    if moving_set.iter().any(|id| is_descendant(nodes, target, id)) {
        return None;
    }
    // The moving nodes are put in the order in which the tree shows them.
    let moving = nodes
        .iter()
        .filter(|node| moving_set.contains(&node.id))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let parent = match position {
        DropPosition::Inside => Some(target.to_owned()),
        _ => target_node.parent.clone(),
    };
    let mut siblings = nodes
        .iter()
        .filter(|node| node.parent == parent && !moving_set.contains(&node.id))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let insert_at = match position {
        DropPosition::Inside => siblings.len(),
        DropPosition::Before => siblings.iter().position(|id| id == target)?,
        DropPosition::After => siblings.iter().position(|id| id == target)? + 1,
    };
    siblings.splice(insert_at..insert_at, moving.iter().cloned());
    Some(Plan {
        moving,
        parent,
        ordered: siblings,
    })
}

#[cfg(test)]
mod tests {
    use super::{Node, Plan, can_take, move_targets, plan_drop, position_at, top_level};
    use crate::ui::sidebar_row::DropPosition;

    /// Returns a tree in which folder 2 is in folder 1, folder 3 is in
    /// folder 2, and folders 4 and 5 are at the top level.
    fn tree() -> Vec<Node> {
        let node = |id: &str, parent: Option<&str>| Node {
            id: id.into(),
            parent: parent.map(Into::into),
        };
        vec![
            node("folder:1", None),
            node("folder:2", Some("folder:1")),
            node("folder:3", Some("folder:2")),
            node("folder:4", None),
            node("folder:5", None),
        ]
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn the_row_is_split_into_before_inside_and_after() {
        assert_eq!(position_at(0.1), DropPosition::Before);
        assert_eq!(position_at(0.5), DropPosition::Inside);
        assert_eq!(position_at(0.9), DropPosition::After);
    }

    #[test]
    fn a_node_cannot_go_into_itself_or_under_itself() {
        let tree = tree();
        assert!(!can_take(&tree, "folder:1", "folder:1"));
        assert!(!can_take(&tree, "folder:1", "folder:3"));
        assert!(can_take(&tree, "folder:3", "folder:1"));
        assert_eq!(
            move_targets(&tree, &ids(&["folder:2"])),
            ids(&["folder:1", "folder:4", "folder:5"])
        );
    }

    #[test]
    fn a_selected_child_moves_with_its_selected_ancestor() {
        let tree = tree();
        assert_eq!(
            top_level(&tree, &ids(&["folder:3", "folder:1", "folder:5"])),
            ids(&["folder:1", "folder:5"])
        );
    }

    #[test]
    fn drops_reorder_and_reparent_as_the_earlier_versions_plan_did() {
        let tree = tree();
        // Folder 5 is dropped before folder 4 at the top level.
        assert_eq!(
            plan_drop(&tree, &[], "folder:5", "folder:4", DropPosition::Before),
            Some(Plan {
                moving: ids(&["folder:5"]),
                parent: None,
                ordered: ids(&["folder:1", "folder:5", "folder:4"]),
            })
        );
        // Folder 4 is dropped inside folder 2 and comes after the children
        // that folder 2 already has.
        assert_eq!(
            plan_drop(&tree, &[], "folder:4", "folder:2", DropPosition::Inside),
            Some(Plan {
                moving: ids(&["folder:4"]),
                parent: Some("folder:2".into()),
                ordered: ids(&["folder:3", "folder:4"]),
            })
        );
        // All selected nodes move when the dragged node is one of them.
        assert_eq!(
            plan_drop(
                &tree,
                &ids(&["folder:5", "folder:4"]),
                "folder:5",
                "folder:1",
                DropPosition::After
            ),
            Some(Plan {
                moving: ids(&["folder:4", "folder:5"]),
                parent: None,
                ordered: ids(&["folder:1", "folder:4", "folder:5"]),
            })
        );
        // A node cannot be dropped on a moving node or under one.
        assert_eq!(
            plan_drop(&tree, &[], "folder:1", "folder:3", DropPosition::After),
            None
        );
    }
}
