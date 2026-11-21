//! Holds the rules that the inspector's tag edits and folder edits share. A
//! click adds one tag or folder to every item the inspector describes, or
//! removes it from every one. Some edits change which items the grid's
//! destination holds.
use picto_library::query::ItemScope;
use std::collections::BTreeSet;

/// Removes `item` from `set` if it is there, and inserts it if it is not.
/// Returns true if it was inserted, which means the items gain the tag or
/// folder, and false if it was removed, which means they lose it.
pub fn toggle<T: Ord>(set: &mut BTreeSet<T>, item: T) -> bool {
    if set.remove(&item) {
        false
    } else {
        set.insert(item);
        true
    }
}

/// Records a click on a tag in the Add Tags dialog or on a folder in the
/// Folders dialog. `members` holds what the items already have. A click on a
/// member puts it in `remove`, and a second click takes it out of `remove`
/// again. A click on anything else does the same with `add`. Both lists keep
/// the order of the clicks.
pub fn mark<T: Ord>(members: &BTreeSet<T>, add: &mut Vec<T>, remove: &mut Vec<T>, item: T) {
    let flip = |list: &mut Vec<T>, item: T| match list.iter().position(|known| *known == item) {
        Some(at) => {
            list.remove(at);
        }
        None => list.push(item),
    };
    if members.contains(&item) {
        add.retain(|known| *known != item);
        flip(remove, item);
    } else {
        flip(add, item);
    }
}

/// Says what kind of data an edit changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touches {
    Tags,
    Folders,
    /// Notes or source addresses. Only the text search reads them.
    Text,
}

/// Returns true if an edit can change which items the grid's destination
/// holds, in which case the grid's items are fetched again. During a search
/// every edit can. Otherwise an edit of tags can for Untagged and for a
/// smart folder, an edit of folders can for Uncategorized, a folder and a
/// smart folder, and both can for a filtered grid.
pub fn changes_results(
    scope: &ItemScope,
    filtered: bool,
    searching: bool,
    touches: Touches,
) -> bool {
    if searching {
        return true;
    }
    match touches {
        Touches::Text => false,
        Touches::Tags => {
            filtered || matches!(scope, ItemScope::Untagged | ItemScope::SmartFolder { .. })
        }
        Touches::Folders => {
            filtered
                || matches!(
                    scope,
                    ItemScope::Uncategorized
                        | ItemScope::Folder { .. }
                        | ItemScope::FolderTree { .. }
                        | ItemScope::SmartFolder { .. }
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Touches, changes_results, mark, toggle};
    use picto_library::FolderId;
    use picto_library::query::ItemScope;
    use std::collections::BTreeSet;

    #[test]
    fn a_click_adds_what_is_missing_and_removes_what_is_there() {
        // `shared` holds the folders that every item of a selection of several
        // items is in. Folder 3 is one of them, so a click removes it.
        let mut shared = BTreeSet::from([3, 7]);
        assert!(!toggle(&mut shared, 3));
        assert_eq!(shared, BTreeSet::from([7]));
        // Folder 5 is not in the set, so a click adds it to every item.
        assert!(toggle(&mut shared, 5));
        assert_eq!(shared, BTreeSet::from([5, 7]));
        // A second click on folder 5 removes it from every item.
        assert!(!toggle(&mut shared, 5));
        let mut names = BTreeSet::new();
        assert!(toggle(&mut names, "species:cat".to_owned()));
    }

    #[test]
    fn a_dialog_click_marks_what_to_add_and_what_the_items_lose() {
        let members = BTreeSet::from(["cat"]);
        let (mut add, mut remove) = (Vec::new(), Vec::new());
        mark(&members, &mut add, &mut remove, "dog");
        mark(&members, &mut add, &mut remove, "cat");
        mark(&members, &mut add, &mut remove, "bird");
        assert_eq!(
            (&add[..], &remove[..]),
            (&["dog", "bird"][..], &["cat"][..])
        );
        // A second click on "dog" and on "cat" takes each out of its list again.
        mark(&members, &mut add, &mut remove, "dog");
        mark(&members, &mut add, &mut remove, "cat");
        assert_eq!((&add[..], remove.is_empty()), (&["bird"][..], true));
    }

    #[test]
    fn only_edits_that_change_membership_fetch_the_grid_again() {
        let folder = ItemScope::FolderTree {
            folder_id: FolderId(4),
        };
        assert!(changes_results(
            &ItemScope::Untagged,
            false,
            false,
            Touches::Tags
        ));
        assert!(!changes_results(
            &ItemScope::All,
            false,
            false,
            Touches::Tags
        ));
        assert!(changes_results(&ItemScope::All, true, false, Touches::Tags));
        assert!(changes_results(&folder, false, false, Touches::Folders));
        assert!(changes_results(
            &ItemScope::Uncategorized,
            false,
            false,
            Touches::Folders
        ));
        assert!(!changes_results(&folder, false, false, Touches::Tags));
        assert!(!changes_results(&folder, true, false, Touches::Text));
        assert!(changes_results(&ItemScope::All, false, true, Touches::Text));
    }
}
