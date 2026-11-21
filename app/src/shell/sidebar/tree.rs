//! The folder and smart-folder trees as the Electron sidebar reasons about
//! them when moving nodes: which nodes can take others, which of a
//! selection actually moves, and where a dragged node lands.
use crate::backend::Navigation;
use crate::ui::sidebar_row::DropPosition;

/// One node of a tree, in the order the sidebar shows siblings.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub parent: Option<String>,
}

/// The nodes of one section (`folder:` or `smart:`), from the destinations.
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

/// `isDescendantOf`: whether `node` sits somewhere under `ancestor`.
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

/// `deduplicateParentChild`: a selected node whose ancestor is also
/// selected moves with that ancestor, so only the topmost ones move.
pub fn top_level(nodes: &[Node], ids: &[String]) -> Vec<String> {
    ids.iter()
        .filter(|id| {
            !ids.iter()
                .any(|other| other != *id && is_descendant(nodes, id, other))
        })
        .cloned()
        .collect()
}

/// `availableTreeMoveTargetIds`: every node that is neither moving nor
/// under a moving node, in tree order.
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

/// `resolveSidebarTreeDrop`: where on a row the pointer is, by the share of
/// the row's height above it: the top 30% drops before, the bottom 30%
/// after, the middle inside.
pub fn position_at(ratio: f32) -> DropPosition {
    if ratio < 0.3 {
        DropPosition::Before
    } else if ratio > 0.7 {
        DropPosition::After
    } else {
        DropPosition::Inside
    }
}

/// Whether `target` can take a drop of `dragged` at all: not itself and not
/// one of its own descendants.
pub fn can_take(nodes: &[Node], dragged: &str, target: &str) -> bool {
    target != dragged && !is_descendant(nodes, target, dragged)
}

/// What a drop does (`SidebarTreeMovePlan`): the nodes that move, the parent
/// they move under (`None` for the top level), and the new order of that
/// parent's children.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub moving: Vec<String>,
    pub parent: Option<String>,
    pub ordered: Vec<String>,
}

/// `planSidebarTreeDrop`: the dragged node moves, or the whole selection
/// when the dragged node is part of a larger one, without the nodes that
/// move with an ancestor. A drop on a moving node or under one does nothing.
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
    // In tree order.
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

    /// 1 ─ 2 ─ 3, then 4 and 5 at the top level.
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
    fn drops_reorder_and_reparent_as_the_electron_plan_does() {
        let tree = tree();
        // 5 before 4 at the top level.
        assert_eq!(
            plan_drop(&tree, &[], "folder:5", "folder:4", DropPosition::Before),
            Some(Plan {
                moving: ids(&["folder:5"]),
                parent: None,
                ordered: ids(&["folder:1", "folder:5", "folder:4"]),
            })
        );
        // 4 inside 2, after its children.
        assert_eq!(
            plan_drop(&tree, &[], "folder:4", "folder:2", DropPosition::Inside),
            Some(Plan {
                moving: ids(&["folder:4"]),
                parent: Some("folder:2".into()),
                ordered: ids(&["folder:3", "folder:4"]),
            })
        );
        // The selection moves when the dragged row is part of it.
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
        // Nothing lands on or under a moving node.
        assert_eq!(
            plan_drop(&tree, &[], "folder:1", "folder:3", DropPosition::After),
            None
        );
    }
}
