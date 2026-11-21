//! What an empty grid says, as `GridScreen.tsx` words it for each scope.
use crate::i18n::{en, t};
use picto_library::query::ItemScope;

#[derive(Debug, PartialEq)]
pub struct EmptyCopy {
    pub title: &'static str,
    pub description: &'static str,
    /// Whether the Import Files and Import Folder buttons show.
    pub import: bool,
}

/// The copy for an empty scope. A search or a filter explains itself first.
pub fn copy(scope: &ItemScope, searching: bool, filtered: bool) -> EmptyCopy {
    let (title, description) = if searching {
        (
            en("No results found"),
            en("Try different search terms or clear filters"),
        )
    } else if filtered {
        (
            t("No items match these filters"),
            t("Try adjusting or clearing your filters"),
        )
    } else {
        match scope {
            ItemScope::Inbox => (
                en("Inbox is empty"),
                en("Run subscriptions to add new images to your inbox"),
            ),
            ItemScope::Uncategorized => (
                en("No uncategorized images"),
                en("All your images are already assigned to folders"),
            ),
            ItemScope::Untagged => (
                en("No untagged images"),
                en("All your images have been tagged"),
            ),
            ItemScope::SmartFolder { .. } => (
                en("No matching images"),
                en("Try adjusting the rules for this smart folder"),
            ),
            ItemScope::MediaMatches { .. } => (
                en("No items use this media"),
                en("This media is no longer used by another library item"),
            ),
            ItemScope::Folder { .. } | ItemScope::FolderTree { .. } => (
                en("This folder is empty"),
                en("Drag and drop files here, or import them below"),
            ),
            ItemScope::All | ItemScope::Trash | ItemScope::RecentlyViewed => (
                en("No images"),
                en("Drag and drop files here, or click the button below to import"),
            ),
        }
    };
    let import = !searching
        && !filtered
        && !matches!(
            scope,
            ItemScope::Inbox
                | ItemScope::Untagged
                | ItemScope::SmartFolder { .. }
                | ItemScope::MediaMatches { .. }
        );
    EmptyCopy {
        title,
        description,
        import,
    }
}

#[cfg(test)]
mod tests {
    use super::copy;
    use picto_library::query::ItemScope;

    #[test]
    fn each_scope_explains_itself() {
        let inbox = copy(&ItemScope::Inbox, false, false);
        assert_eq!(inbox.title, "Inbox is empty");
        assert!(!inbox.import);
        let all = copy(&ItemScope::All, false, false);
        assert_eq!(all.title, "No images");
        assert!(all.import);
        assert!(copy(&ItemScope::Uncategorized, false, false).import);
        assert!(!copy(&ItemScope::Untagged, false, false).import);
    }

    #[test]
    fn a_search_or_a_filter_comes_first_and_hides_import() {
        let searched = copy(&ItemScope::Inbox, true, true);
        assert_eq!(searched.title, "No results found");
        assert!(!searched.import);
        let filtered = copy(&ItemScope::All, false, true);
        assert_eq!(filtered.title, "No items match these filters");
        assert!(!filtered.import);
    }
}
