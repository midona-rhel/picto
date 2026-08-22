//! In-memory projections for the replacement library schema.
//!
//! SQLite remains authoritative. This module only keeps root-scoped Roaring
//! bitmaps and the small amount of canonical state needed to update them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;

use roaring::RoaringBitmap;
use rusqlite::Connection;

pub use crate::app::{ItemKind, Lifecycle};

/// A rebuildable projection of root, folder, membership, and tag state.
pub struct ProjectionStore {
    state: RwLock<State>,
}

#[derive(Debug, Clone, Copy)]
pub struct ItemProjectionChange {
    pub item_id: i64,
    pub kind: ItemKind,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RootProjectionChange {
    pub item_id: i64,
    pub lifecycle: Option<Lifecycle>,
}

#[derive(Debug, Clone, Copy)]
pub struct MembershipProjectionChange {
    pub collection_id: i64,
    pub media_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct FolderProjectionChange {
    pub folder_id: i64,
    pub item_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TagProjectionChange {
    pub media_id: i64,
    pub tag_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StructureProjectionDelta {
    pub items: Vec<ItemProjectionChange>,
    pub roots: Vec<RootProjectionChange>,
    pub memberships: Vec<MembershipProjectionChange>,
    pub folders: Vec<FolderProjectionChange>,
    pub tags: Vec<TagProjectionChange>,
}

#[derive(Default)]
struct State {
    root_lifecycles: HashMap<i64, Lifecycle>,
    lifecycle_bitmaps: [RoaringBitmap; 3],
    media_ids: HashSet<i64>,
    collection_ids: HashSet<i64>,
    collection_members: HashMap<i64, HashSet<i64>>,
    media_to_root: HashMap<i64, i64>,
    folder_members: HashMap<i64, HashSet<i64>>,
    folder_bitmaps: HashMap<i64, RoaringBitmap>,
    direct_tags: HashMap<i64, HashSet<i64>>,
    effective_tags: HashMap<i64, HashSet<i64>>,
    aliases: HashMap<i64, HashSet<i64>>,
    implications: HashMap<i64, HashSet<i64>>,
    direct_tag_bitmaps: HashMap<i64, RoaringBitmap>,
    effective_tag_bitmaps: HashMap<i64, RoaringBitmap>,
}

impl ProjectionStore {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(State::default()),
        }
    }

    /// Build every projection from the replacement schema in one read pass.
    pub fn from_connection(connection: &Connection) -> Result<Self, String> {
        let mut state = State::default();

        {
            let mut statement = connection
                .prepare("SELECT item_id, kind FROM library_item")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (item_id, kind) = row.map_err(|error| error.to_string())?;
                match kind.as_str() {
                    "media" => {
                        state.media_ids.insert(item_id);
                    }
                    "collection" => {
                        state.collection_ids.insert(item_id);
                    }
                    other => return Err(format!("invalid library item kind: {other}")),
                }
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT item_id, lifecycle FROM library_root")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (item_id, lifecycle) = row.map_err(|error| error.to_string())?;
                state
                    .root_lifecycles
                    .insert(item_id, parse_lifecycle(&lifecycle)?);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT collection_id, media_item_id FROM collection_member")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (collection_id, media_id) = row.map_err(|error| error.to_string())?;
                state
                    .collection_members
                    .entry(collection_id)
                    .or_default()
                    .insert(media_id);
                state.media_to_root.insert(media_id, collection_id);
            }
        }

        for media_id in state.media_ids.iter().copied() {
            if !state.media_to_root.contains_key(&media_id)
                && state.root_lifecycles.contains_key(&media_id)
            {
                state.media_to_root.insert(media_id, media_id);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT folder_id, item_id FROM folder_item")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (folder_id, item_id) = row.map_err(|error| error.to_string())?;
                state
                    .folder_members
                    .entry(folder_id)
                    .or_default()
                    .insert(item_id);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT media_item_id, tag_id FROM media_tag")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (media_id, tag_id) = row.map_err(|error| error.to_string())?;
                state
                    .direct_tags
                    .entry(media_id)
                    .or_default()
                    .insert(tag_id);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT from_tag_id, to_tag_id FROM tag_alias")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (from, to) = row.map_err(|error| error.to_string())?;
                add_alias(&mut state, from, to);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT child_tag_id, parent_tag_id FROM tag_implication")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (child, parent) = row.map_err(|error| error.to_string())?;
                state.implications.entry(child).or_default().insert(parent);
            }
        }

        validate_bitmap_ids(&state)?;
        rebuild_all_derived(&mut state);
        Ok(Self {
            state: RwLock::new(state),
        })
    }

    pub fn initialize(connection: &Connection) -> Result<Self, String> {
        Self::from_connection(connection)
    }

    /// Replace all derived state from SQLite. This is the recovery path when
    /// an incremental settlement fails after the authoritative commit.
    pub fn reload(&self, connection: &Connection) -> Result<(), String> {
        let rebuilt = Self::from_connection(connection)?;
        let rebuilt = rebuilt
            .state
            .into_inner()
            .map_err(|_| "Rebuilt projection lock poisoned".to_string())?;
        *self
            .state
            .write()
            .map_err(|_| "Projection lock poisoned".to_string())? = rebuilt;
        Ok(())
    }

    pub fn lifecycle_bitmap(&self, lifecycle: Lifecycle) -> RoaringBitmap {
        self.state.read().unwrap().lifecycle_bitmaps[lifecycle_index(lifecycle)].clone()
    }

    pub fn active_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Active)
    }

    pub fn inbox_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Inbox)
    }

    pub fn trash_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Trash)
    }

    pub fn folder_bitmap(&self, folder_id: i64) -> RoaringBitmap {
        let state = self.state.read().unwrap();
        let folder = state
            .folder_bitmaps
            .get(&folder_id)
            .cloned()
            .unwrap_or_default();
        folder & &state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)]
    }

    pub fn direct_tag_bitmap(&self, tag_id: i64) -> RoaringBitmap {
        self.state
            .read()
            .unwrap()
            .direct_tag_bitmaps
            .get(&tag_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn effective_tag_bitmap(&self, tag_id: i64) -> RoaringBitmap {
        self.state
            .read()
            .unwrap()
            .effective_tag_bitmaps
            .get(&tag_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn root_for_media(&self, media_id: i64) -> Option<i64> {
        self.state
            .read()
            .unwrap()
            .media_to_root
            .get(&media_id)
            .copied()
    }

    /// Settle structural writes under one projection lock. Only affected root
    /// tag and folder bitmaps are rebuilt; SQLite remains authoritative if
    /// validation fails and the Application boundary performs a full reload.
    pub fn apply_structure_delta(&self, delta: StructureProjectionDelta) -> Result<(), String> {
        let mut state = self.state.write().unwrap();
        let mut affected_roots = HashSet::new();
        let mut affected_folders = HashSet::new();
        let mut touched_media = HashSet::new();

        for change in &delta.items {
            validate_id(change.item_id)?;
            if change.present {
                match change.kind {
                    ItemKind::Media => {
                        state.media_ids.insert(change.item_id);
                        touched_media.insert(change.item_id);
                    }
                    ItemKind::Collection => {
                        state.collection_ids.insert(change.item_id);
                    }
                }
            } else {
                state.media_ids.remove(&change.item_id);
                state.collection_ids.remove(&change.item_id);
                state.direct_tags.remove(&change.item_id);
                state.effective_tags.remove(&change.item_id);
                state.media_to_root.remove(&change.item_id);
                if let Some(members) = state.collection_members.remove(&change.item_id) {
                    touched_media.extend(members);
                }
            }
            affected_roots.insert(change.item_id);
        }

        for change in &delta.roots {
            validate_id(change.item_id)?;
            if let Some(lifecycle) = change.lifecycle {
                set_lifecycle(&mut state, change.item_id, lifecycle);
            } else if let Some(previous) = state.root_lifecycles.remove(&change.item_id) {
                state.lifecycle_bitmaps[lifecycle_index(previous)].remove(change.item_id as u32);
            }
            if state.media_ids.contains(&change.item_id) {
                touched_media.insert(change.item_id);
            }
            affected_roots.insert(change.item_id);
        }

        for change in &delta.memberships {
            validate_id(change.collection_id)?;
            validate_id(change.media_id)?;
            if let Some(root_id) = state.media_to_root.get(&change.media_id).copied() {
                affected_roots.insert(root_id);
            }
            if change.present {
                for members in state.collection_members.values_mut() {
                    members.remove(&change.media_id);
                }
                state
                    .collection_members
                    .entry(change.collection_id)
                    .or_default()
                    .insert(change.media_id);
            } else if let Some(members) = state.collection_members.get_mut(&change.collection_id) {
                members.remove(&change.media_id);
            }
            touched_media.insert(change.media_id);
            affected_roots.insert(change.collection_id);
        }

        for media_id in touched_media {
            let collection =
                state
                    .collection_members
                    .iter()
                    .find_map(|(collection_id, members)| {
                        members.contains(&media_id).then_some(*collection_id)
                    });
            if let Some(root_id) = collection {
                state.media_to_root.insert(media_id, root_id);
                affected_roots.insert(root_id);
            } else if state.root_lifecycles.contains_key(&media_id)
                && state.media_ids.contains(&media_id)
            {
                state.media_to_root.insert(media_id, media_id);
                affected_roots.insert(media_id);
            } else {
                state.media_to_root.remove(&media_id);
            }
        }

        for change in delta.folders {
            let members = state.folder_members.entry(change.folder_id).or_default();
            if change.present {
                members.insert(change.item_id);
            } else {
                members.remove(&change.item_id);
            }
            affected_folders.insert(change.folder_id);
        }

        for change in delta.tags {
            let tags = state.direct_tags.entry(change.media_id).or_default();
            if change.present {
                tags.insert(change.tag_id);
            } else {
                tags.remove(&change.tag_id);
            }
            recompute_effective_tags(&mut state, change.media_id);
            if let Some(root_id) = state.media_to_root.get(&change.media_id).copied() {
                affected_roots.insert(root_id);
            }
        }

        for folder_id in affected_folders {
            rebuild_folder_bitmap(&mut state, folder_id);
        }
        rebuild_tag_bitmaps_for_roots(&mut state, &affected_roots.into_iter().collect::<Vec<_>>());
        Ok(())
    }

    /// Apply a lifecycle change to an existing root.
    pub fn apply_lifecycle_delta(&self, item_id: i64, lifecycle: Lifecycle) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.state.write().unwrap();
        if !state.root_lifecycles.contains_key(&item_id) {
            return Err(format!("item {item_id} is not a projection root"));
        }
        set_lifecycle(&mut state, item_id, lifecycle);
        Ok(())
    }

    /// Add, update, or remove a root.
    pub fn apply_root_delta(
        &self,
        item_id: i64,
        kind: ItemKind,
        lifecycle: Option<Lifecycle>,
    ) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.state.write().unwrap();
        match lifecycle {
            Some(lifecycle) => {
                match kind {
                    ItemKind::Media => {
                        state.media_ids.insert(item_id);
                    }
                    ItemKind::Collection => {
                        state.collection_ids.insert(item_id);
                    }
                }
                state.root_lifecycles.insert(item_id, lifecycle);
                if matches!(kind, ItemKind::Media)
                    && !state
                        .collection_members
                        .values()
                        .any(|members| members.contains(&item_id))
                {
                    state.media_to_root.insert(item_id, item_id);
                }
                if matches!(kind, ItemKind::Collection) {
                    let members = state
                        .collection_members
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    for member_id in members {
                        state.media_to_root.insert(member_id, item_id);
                    }
                }
                rebuild_all_derived(&mut state);
            }
            None => {
                state.root_lifecycles.remove(&item_id);
                if state.media_ids.contains(&item_id)
                    && state.media_to_root.get(&item_id) == Some(&item_id)
                {
                    state.media_to_root.remove(&item_id);
                }
                if state.collection_ids.contains(&item_id) {
                    let members = state
                        .collection_members
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    for member_id in members {
                        if state.root_lifecycles.contains_key(&member_id) {
                            state.media_to_root.insert(member_id, member_id);
                        } else {
                            state.media_to_root.remove(&member_id);
                        }
                    }
                }
                rebuild_all_derived(&mut state);
            }
        }
        Ok(())
    }

    pub fn apply_folder_delta(
        &self,
        folder_id: i64,
        item_id: i64,
        present: bool,
    ) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.state.write().unwrap();
        let members = state.folder_members.entry(folder_id).or_default();
        if present {
            members.insert(item_id);
        } else {
            members.remove(&item_id);
        }
        rebuild_folder_bitmap(&mut state, folder_id);
        Ok(())
    }

    /// Apply a direct media tag change and project it to the media's root.
    pub fn apply_tag_delta(&self, media_id: i64, tag_id: i64, present: bool) -> Result<(), String> {
        validate_id(media_id)?;
        let mut state = self.state.write().unwrap();
        let tags = state.direct_tags.entry(media_id).or_default();
        if present {
            tags.insert(tag_id);
        } else {
            tags.remove(&tag_id);
        }
        recompute_effective_tags(&mut state, media_id);
        if let Some(root_id) = state.media_to_root.get(&media_id).copied() {
            rebuild_tag_bitmaps_for_roots(&mut state, &[root_id]);
        }
        Ok(())
    }

    /// Apply a collection membership change and move tag matches between roots.
    pub fn apply_membership_delta(
        &self,
        collection_id: i64,
        media_id: i64,
        present: bool,
    ) -> Result<(), String> {
        validate_id(collection_id)?;
        validate_id(media_id)?;
        let mut state = self.state.write().unwrap();
        let old_root = state.media_to_root.get(&media_id).copied();
        let mut affected_roots = HashSet::new();
        if let Some(root_id) = old_root {
            affected_roots.insert(root_id);
        }

        if present {
            for members in state.collection_members.values_mut() {
                members.remove(&media_id);
            }
            state
                .collection_members
                .entry(collection_id)
                .or_default()
                .insert(media_id);
            state.media_to_root.insert(media_id, collection_id);
            affected_roots.insert(collection_id);
        } else {
            if let Some(members) = state.collection_members.get_mut(&collection_id) {
                members.remove(&media_id);
            }
            let other_collection = state
                .collection_members
                .iter()
                .find_map(|(id, members)| members.contains(&media_id).then_some(*id));
            match other_collection {
                Some(root_id) => {
                    state.media_to_root.insert(media_id, root_id);
                    affected_roots.insert(root_id);
                }
                None if state.root_lifecycles.contains_key(&media_id) => {
                    state.media_to_root.insert(media_id, media_id);
                    affected_roots.insert(media_id);
                }
                None => {
                    state.media_to_root.remove(&media_id);
                }
            }
        }

        let roots = affected_roots.into_iter().collect::<Vec<_>>();
        rebuild_tag_bitmaps_for_roots(&mut state, &roots);
        Ok(())
    }

    pub fn apply_tag_alias_delta(
        &self,
        from_tag_id: i64,
        to_tag_id: i64,
        present: bool,
    ) -> Result<(), String> {
        let mut state = self.state.write().unwrap();
        if present {
            add_alias(&mut state, from_tag_id, to_tag_id);
        } else {
            remove_alias(&mut state, from_tag_id, to_tag_id);
        }
        rebuild_all_tags(&mut state);
        Ok(())
    }

    pub fn apply_tag_implication_delta(
        &self,
        child_tag_id: i64,
        parent_tag_id: i64,
        present: bool,
    ) -> Result<(), String> {
        let mut state = self.state.write().unwrap();
        let parents = state.implications.entry(child_tag_id).or_default();
        if present {
            parents.insert(parent_tag_id);
        } else {
            parents.remove(&parent_tag_id);
        }
        rebuild_all_tags(&mut state);
        Ok(())
    }
}

impl Default for ProjectionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_lifecycle(value: &str) -> Result<Lifecycle, String> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        other => Err(format!("invalid library lifecycle: {other}")),
    }
}

fn validate_id(id: i64) -> Result<(), String> {
    u32::try_from(id)
        .map(|_| ())
        .map_err(|_| format!("item id {id} cannot be represented by RoaringBitmap"))
}

fn validate_bitmap_ids(state: &State) -> Result<(), String> {
    for id in state
        .root_lifecycles
        .keys()
        .chain(state.media_ids.iter())
        .chain(state.collection_ids.iter())
    {
        validate_id(*id)?;
    }
    Ok(())
}

fn set_lifecycle(state: &mut State, item_id: i64, lifecycle: Lifecycle) {
    if let Some(previous) = state.root_lifecycles.insert(item_id, lifecycle) {
        state.lifecycle_bitmaps[lifecycle_index(previous)].remove(item_id as u32);
    }
    state.lifecycle_bitmaps[lifecycle_index(lifecycle)].insert(item_id as u32);
}

fn lifecycle_index(lifecycle: Lifecycle) -> usize {
    match lifecycle {
        Lifecycle::Inbox => 0,
        Lifecycle::Active => 1,
        Lifecycle::Trash => 2,
    }
}

fn add_alias(state: &mut State, from: i64, to: i64) {
    state.aliases.entry(from).or_default().insert(to);
    state.aliases.entry(to).or_default().insert(from);
}

fn remove_alias(state: &mut State, from: i64, to: i64) {
    if let Some(aliases) = state.aliases.get_mut(&from) {
        aliases.remove(&to);
    }
    if let Some(aliases) = state.aliases.get_mut(&to) {
        aliases.remove(&from);
    }
}

fn rebuild_all_derived(state: &mut State) {
    state.lifecycle_bitmaps = std::array::from_fn(|_| RoaringBitmap::new());
    for (item_id, lifecycle) in state.root_lifecycles.clone() {
        state.lifecycle_bitmaps[lifecycle_index(lifecycle)].insert(item_id as u32);
    }

    state.folder_bitmaps.clear();
    for folder_id in state.folder_members.keys().copied().collect::<Vec<_>>() {
        rebuild_folder_bitmap(state, folder_id);
    }
    rebuild_all_tags(state);
}

fn rebuild_folder_bitmap(state: &mut State, folder_id: i64) {
    let members = state
        .folder_members
        .get(&folder_id)
        .cloned()
        .unwrap_or_default();
    let mut bitmap = RoaringBitmap::new();
    for item_id in members {
        if state.root_lifecycles.contains_key(&item_id) {
            bitmap.insert(item_id as u32);
        }
    }
    state.folder_bitmaps.insert(folder_id, bitmap);
}

fn rebuild_all_tags(state: &mut State) {
    state.effective_tags.clear();
    for media_id in state.media_ids.clone() {
        recompute_effective_tags(state, media_id);
    }
    state.direct_tag_bitmaps.clear();
    state.effective_tag_bitmaps.clear();
    for media_id in state.media_ids.iter().copied() {
        let Some(root_id) = state.media_to_root.get(&media_id).copied() else {
            continue;
        };
        let direct_tags = state
            .direct_tags
            .get(&media_id)
            .cloned()
            .unwrap_or_default();
        let effective_tags = state
            .effective_tags
            .get(&media_id)
            .cloned()
            .unwrap_or_default();
        for tag_id in direct_tags {
            state
                .direct_tag_bitmaps
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        }
        for tag_id in effective_tags {
            state
                .effective_tag_bitmaps
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        }
    }
}

fn rebuild_tag_bitmaps_for_roots(state: &mut State, roots: &[i64]) {
    if roots.is_empty() {
        return;
    }
    let roots = roots.iter().copied().collect::<HashSet<_>>();
    for bitmap in state.direct_tag_bitmaps.values_mut() {
        for root_id in &roots {
            bitmap.remove(*root_id as u32);
        }
    }
    for bitmap in state.effective_tag_bitmaps.values_mut() {
        for root_id in &roots {
            bitmap.remove(*root_id as u32);
        }
    }
    for media_id in state.media_ids.iter().copied() {
        let Some(root_id) = state.media_to_root.get(&media_id).copied() else {
            continue;
        };
        if !roots.contains(&root_id) {
            continue;
        }
        let direct_tags = state
            .direct_tags
            .get(&media_id)
            .cloned()
            .unwrap_or_default();
        let effective_tags = state
            .effective_tags
            .get(&media_id)
            .cloned()
            .unwrap_or_default();
        for tag_id in direct_tags {
            state
                .direct_tag_bitmaps
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        }
        for tag_id in effective_tags {
            state
                .effective_tag_bitmaps
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        }
    }
}

fn recompute_effective_tags(state: &mut State, media_id: i64) {
    let mut effective = HashSet::new();
    let mut queue = VecDeque::new();
    for tag_id in state.direct_tags.get(&media_id).into_iter().flatten() {
        if effective.insert(*tag_id) {
            queue.push_back(*tag_id);
        }
    }
    while let Some(tag_id) = queue.pop_front() {
        for next in state.aliases.get(&tag_id).into_iter().flatten() {
            if effective.insert(*next) {
                queue.push_back(*next);
            }
        }
        for next in state.implications.get(&tag_id).into_iter().flatten() {
            if effective.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    state.effective_tags.insert(media_id, effective);
}

#[cfg(test)]
mod tests {
    use roaring::RoaringBitmap;
    use rusqlite::Connection;

    use super::{ItemKind, Lifecycle, ProjectionStore};

    fn fixture() -> (Connection, ProjectionStore) {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::store::schema::create(&mut connection).unwrap();
        connection
            .execute_batch(
                "
                INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
                    VALUES (1, 'a', 'image/png', 1, 'now'),
                           (2, 'b', 'image/png', 1, 'now'),
                           (3, 'c', 'image/png', 1, 'now');
                INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                    VALUES (10, 'media-a', 'media', 'now', 'now'),
                           (11, 'media-b', 'media', 'now', 'now'),
                           (12, 'media-c', 'media', 'now', 'now'),
                           (20, 'collection-a', 'collection', 'now', 'now');
                INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                    VALUES (10, 1, 'now', 'now'), (11, 2, 'now', 'now'), (12, 3, 'now', 'now');
                INSERT INTO library_root (item_id, lifecycle)
                    VALUES (20, 'active'), (11, 'active'), (12, 'inbox');
                INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                    VALUES (20, 10, 1);
                INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                    VALUES (7, 'folder-a', 'A', 'now', 'now');
                INSERT INTO folder_item (folder_id, item_id) VALUES (7, 20);
                INSERT INTO tag (tag_id, subtag) VALUES (100, 'child'), (101, 'parent');
                INSERT INTO media_tag (media_item_id, tag_id) VALUES (10, 100);
                INSERT INTO tag_implication (child_tag_id, parent_tag_id)
                    VALUES (100, 101);
                ",
            )
            .unwrap();
        let projection = ProjectionStore::from_connection(&connection).unwrap();
        (connection, projection)
    }

    #[test]
    fn full_initialization_uses_collection_and_standalone_root_ids() {
        let (_connection, projection) = fixture();

        assert_eq!(projection.root_for_media(10), Some(20));
        assert_eq!(projection.root_for_media(11), Some(11));
        assert_eq!(
            projection.active_bitmap(),
            RoaringBitmap::from_iter([11, 20])
        );
        assert_eq!(projection.inbox_bitmap(), RoaringBitmap::from_iter([12]));
        assert!(projection.trash_bitmap().is_empty());
        assert_eq!(projection.folder_bitmap(7), RoaringBitmap::from_iter([20]));
    }

    #[test]
    fn direct_and_effective_tag_matches_project_to_collection_root() {
        let (_connection, projection) = fixture();

        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );
        assert_eq!(
            projection.effective_tag_bitmap(101),
            RoaringBitmap::from_iter([20])
        );
        assert!(!projection.effective_tag_bitmap(100).contains(10));
    }

    #[test]
    fn incremental_deltas_move_root_scoped_matches() {
        let (_connection, projection) = fixture();

        projection
            .apply_lifecycle_delta(11, Lifecycle::Trash)
            .unwrap();
        assert_eq!(projection.trash_bitmap(), RoaringBitmap::from_iter([11]));

        projection.apply_tag_delta(11, 100, true).unwrap();
        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([11, 20])
        );

        projection
            .apply_root_delta(11, ItemKind::Media, None)
            .unwrap();
        projection.apply_membership_delta(20, 11, true).unwrap();
        assert_eq!(projection.root_for_media(11), Some(20));
        assert_eq!(
            projection.effective_tag_bitmap(101),
            RoaringBitmap::from_iter([20])
        );
        assert!(!projection.active_bitmap().contains(10));
    }

    #[test]
    fn folder_projection_excludes_non_active_roots() {
        let (connection, projection) = fixture();
        connection
            .execute(
                "INSERT INTO folder_item (folder_id, item_id) VALUES (7, 12)",
                [],
            )
            .unwrap();
        projection.reload(&connection).unwrap();

        assert_eq!(projection.folder_bitmap(7), RoaringBitmap::from_iter([20]));
    }
}
