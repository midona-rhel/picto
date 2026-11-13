//! The rules the inspector's tag and folder edits share: a click adds or
//! removes one tag or folder for every item the inspector describes, and
//! some edits change what the grid's destination holds.
use picto_library::query::ItemScope;
use std::collections::BTreeSet;

/// Takes `item` out of `set`, or puts it in; true when it went in, which is
/// whether the items gain it or lose it.
pub fn toggle<T: Ord>(set: &mut BTreeSet<T>, item: T) -> bool {
    if set.remove(&item) {
        false
    } else {
        set.insert(item);
        true
    }
}

/// What an edit changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Touches {
    Tags,
    Folders,
    /// Notes or source addresses, which only the text search reads.
    Text,
}

/// Whether an edit can change which items the grid's destination holds, so
/// it is fetched again: Untagged by tags, a folder and Uncategorized by
/// memberships, a smart folder and a filtered grid by either, and a search
/// by anything it reads.
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
    use super::{Touches, changes_results, toggle};
    use picto_library::FolderId;
    use picto_library::query::ItemScope;
    use std::collections::BTreeSet;

    #[test]
    fn a_click_adds_what_is_missing_and_removes_what_is_there() {
        // A multi-selection's shared folders: 3 is on every item.
        let mut shared = BTreeSet::from([3, 7]);
        assert!(!toggle(&mut shared, 3));
        assert_eq!(shared, BTreeSet::from([7]));
        // 5 was on some items or none: a click puts it on every one.
        assert!(toggle(&mut shared, 5));
        assert_eq!(shared, BTreeSet::from([5, 7]));
        // Clicking again takes it off every one.
        assert!(!toggle(&mut shared, 5));
        let mut names = BTreeSet::new();
        assert!(toggle(&mut names, "species:cat".to_owned()));
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
