//! Backend-side mutation receipt builders.
//!
//! This is the backend-only compatibility layer that shapes mutation facts
//! before `events.rs` emits the transport event.

use crate::runtime_contract::mutation::{Domain, SidebarCounts};

#[derive(Debug, Clone, Default)]
pub struct MutationImpact {
    pub domains: Vec<Domain>,
    pub file_hashes: Option<Vec<String>>,
    pub folder_ids: Option<Vec<i64>>,
    pub smart_folder_ids: Option<Vec<i64>>,
    pub compiler_batch_done: Option<bool>,
    pub sidebar_counts: Option<SidebarCounts>,
    pub status_changed: Option<bool>,
    pub tags_changed: Option<bool>,
    pub tag_structure_changed: Option<bool>,
    pub folder_membership_changed: Option<Vec<i64>>,
    pub view_prefs_changed: Option<bool>,
    pub extra_grid_scopes: Option<Vec<String>>,
}

impl MutationImpact {
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

    pub fn file_hashes(mut self, hashes: Vec<String>) -> Self {
        self.file_hashes = Some(hashes);
        self
    }

    pub fn folder_ids(mut self, ids: Vec<i64>) -> Self {
        self.folder_ids = Some(ids);
        self
    }

    pub fn smart_folder_ids(mut self, ids: Vec<i64>) -> Self {
        self.smart_folder_ids = Some(ids);
        self
    }

    pub fn sidebar_counts_from(mut self, db: &crate::sqlite::SqliteDatabase) -> Self {
        self.sidebar_counts = Some(sidebar_counts_from_bitmaps(db));
        self
    }

    pub fn status_changed(mut self) -> Self {
        self.status_changed = Some(true);
        self
    }

    pub fn tags_changed(mut self) -> Self {
        self.tags_changed = Some(true);
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

    pub fn extra_grid_scopes(mut self, scopes: Vec<String>) -> Self {
        self.extra_grid_scopes = Some(scopes);
        self
    }

    pub fn file_lifecycle(db: &crate::sqlite::SqliteDatabase) -> Self {
        Self::new()
            .domains(&[Domain::Files, Domain::Sidebar, Domain::SmartFolders])
            .status_changed()
            .sidebar_counts_from(db)
    }

    pub fn file_metadata(hash: String) -> Self {
        Self::new()
            .domains(&[Domain::Files])
            .file_hashes(vec![hash])
    }

    pub fn file_tags(hash: String) -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files])
            .tags_changed()
            .file_hashes(vec![hash])
    }

    pub fn batch_tags() -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files])
            .tags_changed()
    }

    pub fn sidebar(domain: Domain) -> Self {
        Self::new().domains(&[domain, Domain::Sidebar])
    }

    pub fn subscriptions_sidebar() -> Self {
        Self::sidebar(Domain::Subscriptions)
    }

    pub fn file_status_change(db: &crate::sqlite::SqliteDatabase) -> Self {
        Self::new()
            .domains(&[
                Domain::Files,
                Domain::Sidebar,
                Domain::Folders,
                Domain::SmartFolders,
                Domain::Selection,
            ])
            .status_changed()
            .sidebar_counts_from(db)
    }

    pub fn subscriptions_file_status_change(db: &crate::sqlite::SqliteDatabase) -> Self {
        Self::new()
            .domains(&[
                Domain::Subscriptions,
                Domain::Files,
                Domain::Sidebar,
                Domain::Folders,
                Domain::SmartFolders,
                Domain::Selection,
            ])
            .status_changed()
            .sidebar_counts_from(db)
    }

    pub fn folder_file_change(folder_id: i64) -> Self {
        Self::new()
            .domains(&[
                Domain::Folders,
                Domain::Files,
                Domain::Selection,
                Domain::Sidebar,
            ])
            .folder_ids(vec![folder_id])
            .folder_membership_changed(vec![folder_id])
    }

    pub fn tag_structure_change() -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Sidebar, Domain::SmartFolders])
            .tag_structure_changed_fact()
    }

    pub fn folder_item_reorder(folder_id: i64) -> Self {
        Self::new()
            .domains(&[Domain::Folders])
            .folder_ids(vec![folder_id])
    }

    pub fn selection_batch_tags() -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files, Domain::Selection])
            .tags_changed()
    }

    pub fn collection_update(collection_id: i64) -> Self {
        Self::new().extra_grid_scopes(vec![
            format!("collection:{collection_id}"),
            "system:all".into(),
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

    pub fn collection_members_reordered(collection_id: i64) -> Self {
        Self::new().extra_grid_scopes(vec![
            format!("collection:{collection_id}"),
            "system:all".into(),
        ])
    }

    pub fn selection_metadata() -> Self {
        Self::new().domains(&[Domain::Files, Domain::Selection])
    }

    pub fn selection_metadata_grid() -> Self {
        Self::new().domains(&[Domain::Files]).tags_changed()
    }

    pub fn view_prefs_change() -> Self {
        Self::new()
            .domains(&[Domain::ViewPrefs])
            .view_prefs_changed()
    }

    pub fn compiler_publish(sidebar_affected: bool, smart_folders_rebuilt: bool) -> Self {
        let mut impact = Self::new();
        if sidebar_affected {
            impact = impact.add_domain(Domain::Sidebar);
        }
        impact.compiler_batch_done = Some(true);
        if smart_folders_rebuilt {
            impact = impact.extra_grid_scopes(vec!["system:all".into()]);
        }
        impact
    }
}

pub fn sidebar_counts_from_bitmaps(db: &crate::sqlite::SqliteDatabase) -> SidebarCounts {
    use crate::sqlite::bitmaps::BitmapKey;

    SidebarCounts {
        all_active: db.bitmaps.len(&BitmapKey::Status(1)) as i64,
        inbox: db.bitmaps.len(&BitmapKey::Status(0)) as i64,
        trash: db.bitmaps.len(&BitmapKey::Status(2)) as i64,
    }
}
