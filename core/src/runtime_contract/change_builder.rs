//! Backend-side state-change builders.
//!
//! These helpers shape backend change details before `events.rs` emits the
//! transport event.

use crate::runtime_contract::state_change::{
    Domain, MediaDerivativeField, MediaMetadataField, SidebarCounts, TagChangeDetails,
};

#[derive(Debug, Clone, Default)]
pub struct ChangeImpact {
    pub domains: Vec<Domain>,
    pub entity_hashes: Option<Vec<String>>,
    pub member_hashes: Option<Vec<String>>,
    pub folder_ids: Option<Vec<i64>>,
    pub smart_folder_ids: Option<Vec<i64>>,
    pub compiler_batch_done: Option<bool>,
    pub sidebar_counts: Option<SidebarCounts>,
    pub status_changed: Option<bool>,
    pub tags_changed: Option<bool>,
    pub tag_changes: Option<TagChangeDetails>,
    pub tag_structure_changed: Option<bool>,
    pub folder_membership_changed: Option<Vec<i64>>,
    pub view_prefs_changed: Option<bool>,
    pub media_metadata_changed: Option<bool>,
    pub media_fields_changed: Option<Vec<MediaMetadataField>>,
    pub media_derivatives_changed: Option<bool>,
    pub derivative_fields_changed: Option<Vec<MediaDerivativeField>>,
    pub extra_grid_scopes: Option<Vec<String>>,
    pub group_ids: Option<Vec<i64>>,
    pub subscription_ids: Option<Vec<i64>>,
    pub query_ids: Option<Vec<i64>>,
    pub credential_categories: Option<Vec<String>>,
    pub folder_parent_changes: Option<Vec<(i64, Option<i64>)>>,
    pub folder_order_changes: Option<Vec<(i64, i64)>>,
    pub smart_folder_parent_changes: Option<Vec<(i64, Option<i64>)>>,
    pub smart_folder_order_changes: Option<Vec<(i64, i64)>>,
}

impl ChangeImpact {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn domains(mut self, domains: &[Domain]) -> Self {
        let mut unique = Vec::with_capacity(domains.len());
        for domain in domains {
            if !unique.contains(domain) {
                unique.push(domain.clone());
            }
        }
        self.domains = unique;
        self
    }

    pub fn add_domain(mut self, domain: Domain) -> Self {
        if !self.domains.contains(&domain) {
            self.domains.push(domain);
        }
        self
    }

    pub fn add_domains(mut self, domains: &[Domain]) -> Self {
        for domain in domains {
            if !self.domains.contains(domain) {
                self.domains.push(domain.clone());
            }
        }
        self
    }

    pub fn entity_hashes(mut self, hashes: Vec<String>) -> Self {
        self.entity_hashes = Some(hashes);
        self
    }

    pub fn member_hashes(mut self, hashes: Vec<String>) -> Self {
        self.member_hashes = Some(hashes);
        self
    }

    pub fn folder_ids(mut self, ids: Vec<i64>) -> Self {
        let merged = self.folder_ids.get_or_insert_with(Vec::new);
        for id in ids {
            if !merged.contains(&id) {
                merged.push(id);
            }
        }
        self
    }

    pub fn smart_folder_ids(mut self, ids: Vec<i64>) -> Self {
        let merged = self.smart_folder_ids.get_or_insert_with(Vec::new);
        for id in ids {
            if !merged.contains(&id) {
                merged.push(id);
            }
        }
        self
    }

    pub fn group_ids(mut self, ids: Vec<i64>) -> Self {
        let merged = self.group_ids.get_or_insert_with(Vec::new);
        for id in ids {
            if !merged.contains(&id) { merged.push(id); }
        }
        self
    }

    pub fn subscription_ids(mut self, ids: Vec<i64>) -> Self {
        let merged = self.subscription_ids.get_or_insert_with(Vec::new);
        for id in ids {
            if !merged.contains(&id) { merged.push(id); }
        }
        self
    }

    pub fn query_ids(mut self, ids: Vec<i64>) -> Self {
        let merged = self.query_ids.get_or_insert_with(Vec::new);
        for id in ids {
            if !merged.contains(&id) { merged.push(id); }
        }
        self
    }

    pub fn credential_categories(mut self, cats: Vec<String>) -> Self {
        let merged = self.credential_categories.get_or_insert_with(Vec::new);
        for cat in cats {
            if !merged.contains(&cat) { merged.push(cat); }
        }
        self
    }

    pub fn folder_parent_changes(mut self, changes: Vec<(i64, Option<i64>)>) -> Self {
        self.folder_parent_changes = Some(changes);
        self
    }

    pub fn folder_order_changes(mut self, changes: Vec<(i64, i64)>) -> Self {
        self.folder_order_changes = Some(changes);
        self
    }

    pub fn smart_folder_parent_changes(mut self, changes: Vec<(i64, Option<i64>)>) -> Self {
        self.smart_folder_parent_changes = Some(changes);
        self
    }

    pub fn smart_folder_order_changes(mut self, changes: Vec<(i64, i64)>) -> Self {
        self.smart_folder_order_changes = Some(changes);
        self
    }

    pub fn merge(mut self, other: Self) -> Self {
        self = self.add_domains(&other.domains);

        if let Some(hashes) = other.entity_hashes {
            let merged = self.entity_hashes.get_or_insert_with(Vec::new);
            for hash in hashes {
                if !merged.contains(&hash) {
                    merged.push(hash);
                }
            }
        }

        if let Some(hashes) = other.member_hashes {
            let merged = self.member_hashes.get_or_insert_with(Vec::new);
            for hash in hashes {
                if !merged.contains(&hash) {
                    merged.push(hash);
                }
            }
        }

        if let Some(ids) = other.folder_ids {
            self = self.folder_ids(ids);
        }

        if let Some(ids) = other.smart_folder_ids {
            self = self.smart_folder_ids(ids);
        }

        if other.compiler_batch_done == Some(true) {
            self.compiler_batch_done = Some(true);
        }

        if other.status_changed == Some(true) {
            self.status_changed = Some(true);
        }

        if other.tags_changed == Some(true) {
            self.tags_changed = Some(true);
        }

        if let Some(tag_changes) = other.tag_changes {
            if let Some(added) = tag_changes.added {
                self = self.tags_added(added);
            }
            if let Some(removed) = tag_changes.removed {
                self = self.tags_removed(removed);
            }
        }

        if other.tag_structure_changed == Some(true) {
            self.tag_structure_changed = Some(true);
        }

        if let Some(ids) = other.folder_membership_changed {
            let merged = self.folder_membership_changed.get_or_insert_with(Vec::new);
            for id in ids {
                if !merged.contains(&id) {
                    merged.push(id);
                }
            }
        }

        if other.view_prefs_changed == Some(true) {
            self.view_prefs_changed = Some(true);
        }

        if other.media_metadata_changed == Some(true) {
            self.media_metadata_changed = Some(true);
        }

        if let Some(fields) = other.media_fields_changed {
            self = self.media_fields_changed(&fields);
        }

        if other.media_derivatives_changed == Some(true) {
            self.media_derivatives_changed = Some(true);
        }

        if let Some(fields) = other.derivative_fields_changed {
            self = self.derivative_fields_changed(&fields);
        }

        if let Some(scopes) = other.extra_grid_scopes {
            self = self.extra_grid_scopes(scopes);
        }

        if let Some(ids) = other.group_ids {
            self = self.group_ids(ids);
        }
        if let Some(ids) = other.subscription_ids {
            self = self.subscription_ids(ids);
        }
        if let Some(ids) = other.query_ids {
            self = self.query_ids(ids);
        }
        if let Some(cats) = other.credential_categories {
            self = self.credential_categories(cats);
        }

        if other.sidebar_counts.is_some() {
            self.sidebar_counts = other.sidebar_counts;
        }

        self
    }

    fn merge_extra_grid_scopes(&mut self, scopes: Vec<String>) {
        let merged = self.extra_grid_scopes.get_or_insert_with(Vec::new);
        for scope in scopes {
            if !merged.contains(&scope) {
                merged.push(scope);
            }
        }
    }

    pub fn sidebar_counts_from(mut self, db: &crate::sqlite::SqliteDatabase) -> Self {
        self.sidebar_counts = Some(sidebar_counts_from_bitmaps(db));
        self
    }

    pub fn status_changed(mut self) -> Self {
        self.status_changed = Some(true);
        self
    }

    pub fn status_sensitive_grid_scopes_changed(mut self) -> Self {
        self = self.add_domains(&[Domain::Sidebar, Domain::SmartFolders]);
        self.merge_extra_grid_scopes(vec![
            "system:active".into(),
            "system:inbox".into(),
            "system:trash".into(),
            "system:untagged".into(),
            "system:uncategorized".into(),
            "smart:all".into(),
        ]);
        self
    }

    pub fn tags_changed(mut self) -> Self {
        self.tags_changed = Some(true);
        self
    }

    pub fn tags_added(mut self, tags: Vec<String>) -> Self {
        self.tags_changed = Some(true);
        let tag_changes = self
            .tag_changes
            .get_or_insert_with(TagChangeDetails::default);
        let added = tag_changes.added.get_or_insert_with(Vec::new);
        for tag in tags {
            if !added.contains(&tag) {
                added.push(tag);
            }
        }
        self
    }

    pub fn tags_removed(mut self, tags: Vec<String>) -> Self {
        self.tags_changed = Some(true);
        let tag_changes = self
            .tag_changes
            .get_or_insert_with(TagChangeDetails::default);
        let removed = tag_changes.removed.get_or_insert_with(Vec::new);
        for tag in tags {
            if !removed.contains(&tag) {
                removed.push(tag);
            }
        }
        self
    }

    pub fn tag_structure_changed_fact(mut self) -> Self {
        self.tag_structure_changed = Some(true);
        self
    }

    pub fn folder_membership_changed(mut self, ids: Vec<i64>) -> Self {
        self.folder_membership_changed = Some(ids);
        self
    }

    pub fn view_prefs_changed(mut self) -> Self {
        self.view_prefs_changed = Some(true);
        self
    }

    pub fn media_metadata_changed(mut self) -> Self {
        self.media_metadata_changed = Some(true);
        self
    }

    pub fn media_fields_changed(mut self, fields: &[MediaMetadataField]) -> Self {
        self.media_metadata_changed = Some(true);
        let changed = self.media_fields_changed.get_or_insert_with(Vec::new);
        for field in fields {
            if !changed.contains(field) {
                changed.push(field.clone());
            }
        }
        self
    }

    pub fn media_derivatives_changed(mut self) -> Self {
        self.media_derivatives_changed = Some(true);
        self
    }

    pub fn derivative_fields_changed(mut self, fields: &[MediaDerivativeField]) -> Self {
        self.media_derivatives_changed = Some(true);
        let changed = self.derivative_fields_changed.get_or_insert_with(Vec::new);
        for field in fields {
            if !changed.contains(field) {
                changed.push(field.clone());
            }
        }
        self
    }

    pub fn extra_grid_scopes(mut self, scopes: Vec<String>) -> Self {
        self.merge_extra_grid_scopes(scopes);
        self
    }

    pub fn all_smart_folder_scopes_changed(mut self) -> Self {
        self = self.add_domains(&[Domain::SmartFolders, Domain::Sidebar]);
        self.merge_extra_grid_scopes(vec!["smart:all".into()]);
        self
    }

    pub fn smart_folder_scopes_changed_for_media_fields(
        mut self,
        fields: &[MediaMetadataField],
    ) -> Self {
        if fields.iter().any(|field| {
            matches!(
                field,
                MediaMetadataField::Name
                    | MediaMetadataField::Rating
                    | MediaMetadataField::Notes
                    | MediaMetadataField::SourceUrls
            )
        }) {
            self = self.all_smart_folder_scopes_changed();
        }
        self
    }

    pub fn smart_folder_scopes_changed_for_derivative_fields(
        mut self,
        fields: &[MediaDerivativeField],
    ) -> Self {
        if fields
            .iter()
            .any(|field| matches!(field, MediaDerivativeField::DominantColorHex))
        {
            self = self.all_smart_folder_scopes_changed();
        }
        self
    }

    pub fn file_lifecycle(db: &crate::sqlite::SqliteDatabase) -> Self {
        Self::new()
            .status_changed()
            .status_sensitive_grid_scopes_changed()
            .sidebar_counts_from(db)
    }

    pub fn file_metadata(hash: String) -> Self {
        Self::new().entity_hashes(vec![hash]).media_metadata_changed()
    }

    pub fn file_tags(hash: String) -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files, Domain::Selection])
            .tags_changed()
            .entity_hashes(vec![hash])
            .extra_grid_scopes(vec!["system:untagged".into()])
            .all_smart_folder_scopes_changed()
    }

    pub fn batch_tags() -> Self {
        Self::new()
            .domains(&[Domain::Selection])
            .tags_changed()
            .extra_grid_scopes(vec!["system:untagged".into()])
            .all_smart_folder_scopes_changed()
    }

    pub fn folder_file_change(folder_id: i64) -> Self {
        Self::new()
            .add_domain(Domain::Folders)
            .folder_ids(vec![folder_id])
            .folder_membership_changed(vec![folder_id])
    }

    pub fn tag_structure_change() -> Self {
        Self::new().tag_structure_changed_fact()
    }

    pub fn folder_item_reorder(folder_id: i64) -> Self {
        Self::new()
            .add_domain(Domain::Folders)
            .folder_ids(vec![folder_id])
    }


    pub fn collection_update(collection_id: i64) -> Self {
        Self::new().extra_grid_scopes(vec![
            format!("collection:{collection_id}"),
            "system:active".into(),
        ])
    }

    pub fn collection_membership_change(collection_id: i64) -> Self {
        Self::new()
            .folder_membership_changed(vec![collection_id])
            .extra_grid_scopes(vec![
                format!("collection:{collection_id}"),
                "folder:all".into(),
            ])
    }

    pub fn collection_delete(collection_id: i64, folder_ids: Vec<i64>) -> Self {
        let mut impact = Self::new().extra_grid_scopes(vec![
            format!("collection:{collection_id}"),
            "system:active".into(),
        ]);
        if !folder_ids.is_empty() {
            impact = impact.folder_membership_changed(folder_ids);
        }
        impact
    }

    pub fn selection_metadata() -> Self {
        Self::new()
            .domains(&[Domain::Files, Domain::Selection])
            .media_metadata_changed()
    }

    pub fn view_prefs_change() -> Self {
        Self::new().view_prefs_changed()
    }

    pub fn compiler_publish(smart_folders_rebuilt: bool) -> Self {
        let mut impact = Self::new();
        impact.compiler_batch_done = Some(true);
        if smart_folders_rebuilt {
            impact = impact.extra_grid_scopes(vec!["system:active".into()]);
        }
        impact
    }
}

pub fn sidebar_counts_from_bitmaps(db: &crate::sqlite::SqliteDatabase) -> SidebarCounts {
    use crate::sqlite::bitmaps::BitmapKey;

    // Status bitmaps already exclude collection members (the compiler
    // filters them out). Direct bitmap reads give tile-level counts.
    SidebarCounts {
        active: db.bitmaps.len(&BitmapKey::Status(1)) as i64,
        inbox: db.bitmaps.len(&BitmapKey::Status(0)) as i64,
        trash: db.bitmaps.len(&BitmapKey::Status(2)) as i64,
    }
}
