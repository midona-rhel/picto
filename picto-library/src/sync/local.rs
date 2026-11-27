//! What the library holds, in sync keys: the key and the state of each local
//! entity, in the same form as `values::View`.

use std::collections::{BTreeSet, HashMap};

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use super::values::View;
use super::SyncKind;
use crate::model::{Lifecycle, Rating, RootId};
use crate::predicate::ViewQuerySpec;
use crate::projection::ProjectionSnapshot;
use crate::Result;

/// The field of the `application` setting that lists the views of Quick
/// Access in the sidebar, by scope.
pub(crate) const QUICK_ACCESS: &str = "sidebarQuickAccess";

/// The value of the field `decision` of a "Not duplicate" decision.
pub(crate) const KEEP_BOTH: &str = "keep_both";

/// The fields of the `application` setting that stay on each computer,
/// because they describe the computer rather than the library: the window
/// zoom, the window background effects, trackpad gestures, automatic import
/// of watched folders, and the switches of AI tagging. The Subscriptions
/// entry of the sidebar stays too, because subscriptions do not sync.
const LOCAL_SETTINGS: [&str; 12] = [
    "zoomFactor",
    "frostedGlassRails",
    "acrylicRails",
    "viewerTrackpadGestures",
    "autoImportEnabled",
    "aiTaggerWd14Enabled",
    "aiTaggerE621Enabled",
    "aiTaggerEva02Enabled",
    "aiTaggerOppaiOracleEnabled",
    "aiTaggerAutoOnImport",
    "aiTaggerWriteRating",
    "showSidebarSubscriptions",
];

/// Whether the field `name` of the `application` setting stays on this
/// computer: the fields of `LOCAL_SETTINGS`, and the subscription fields,
/// whose names start with `subscription`, because subscriptions do not
/// sync. Every other field syncs.
pub(crate) fn local_setting(name: &str) -> bool {
    name.starts_with("subscription") || LOCAL_SETTINGS.contains(&name)
}

pub(crate) fn lifecycle_name(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Inbox => "inbox",
        Lifecycle::Trash => "trash",
    }
}

pub(crate) fn lifecycle_from_name(name: &str) -> Lifecycle {
    match name {
        "active" => Lifecycle::Active,
        "trash" => Lifecycle::Trash,
        _ => Lifecycle::Inbox,
    }
}

pub(crate) fn rating_from_number(value: &Value) -> Rating {
    Rating::ALL
        .into_iter()
        .find(|rating| Some(u64::from(rating.bitmap_key())) == value.as_u64())
        .unwrap_or(Rating::Unrated)
}

/// Reads the library's entities in sync keys. Tags and folders of roots are
/// found from the projection; `prepare` reads them for many roots at once.
pub(crate) struct Local<'a> {
    pub connection: &'a Connection,
    pub snapshot: &'a ProjectionSnapshot,
    /// The name of each tag id, built when it is first needed, because most
    /// writes read no tags.
    tag_names: Option<HashMap<u32, String>>,
    root_tags: HashMap<u32, Vec<String>>,
    root_folders: HashMap<u32, Vec<String>>,
    prepared: RoaringBitmap,
}

impl<'a> Local<'a> {
    pub(crate) fn new(connection: &'a Connection, snapshot: &'a ProjectionSnapshot) -> Self {
        Self {
            connection,
            snapshot,
            tag_names: None,
            root_tags: HashMap::new(),
            root_folders: HashMap::new(),
            prepared: RoaringBitmap::new(),
        }
    }

    fn tag_names(&mut self) -> &HashMap<u32, String> {
        let snapshot = self.snapshot;
        self.tag_names.get_or_insert_with(|| {
            snapshot
                .tag_ids_by_name
                .iter()
                .map(|(name, id)| (id.0, name.clone()))
                .collect()
        })
    }

    pub(crate) fn folder_key(&self, folder_id: u32) -> Result<Option<String>> {
        self.key(SyncKind::Folder, folder_id)
    }

    /// Reads the tags and folders of `roots` with one pass over the tags and
    /// the folders of the projection.
    pub(crate) fn prepare(&mut self, roots: &RoaringBitmap) -> Result<()> {
        let roots = roots - &self.prepared;
        if roots.is_empty() {
            return Ok(());
        }
        let snapshot = self.snapshot;
        // The tags that hold one of the roots are found first, and only
        // their names are read, so that a write of a few roots does not copy
        // the name of every tag in the library. A library has tens of
        // thousands of tags, so they are checked on the thread pool of the
        // smart folders, as `capture_structure` checks them.
        let entries = snapshot.tags.iter().collect::<Vec<_>>();
        let scan = |(tag_id, members): &(&crate::TagId, &crate::projection::SharedBitmap)| {
            (!members.is_disjoint(&roots)).then(|| (tag_id.0, &***members & &roots))
        };
        let held: HashMap<u32, RoaringBitmap> = match crate::smart::smart_folder_pool() {
            Ok(pool) if entries.len() >= 1_024 => {
                use rayon::prelude::*;
                pool.install(|| entries.par_iter().filter_map(scan).collect())
            }
            _ => entries.iter().filter_map(scan).collect(),
        };
        if !held.is_empty() {
            for (name, tag_id) in snapshot.tag_ids_by_name.iter() {
                for root in held.get(&tag_id.0).into_iter().flatten() {
                    self.root_tags.entry(root).or_default().push(name.clone());
                }
            }
        }
        for (folder_id, members) in self.snapshot.folders.iter() {
            if members.is_disjoint(&roots) {
                continue;
            }
            if let Some(key) = self.folder_key(folder_id.0)? {
                for root in members & &roots {
                    self.root_folders.entry(root).or_default().push(key.clone());
                }
            }
        }
        self.prepared |= roots;
        Ok(())
    }

    /// The kind of a local entity, read from the library's tables.
    pub(crate) fn kind(&self, local_id: u32) -> Result<Option<SyncKind>> {
        Ok(self
            .connection
            .prepare_cached(
                "SELECT CASE
                     WHEN EXISTS(SELECT 1 FROM tag_definition WHERE tag_id = ?1) THEN 'tag'
                     WHEN EXISTS(SELECT 1 FROM tag_namespace WHERE namespace_id = ?1) THEN 'tag_group'
                     WHEN EXISTS(SELECT 1 FROM folder_definition WHERE folder_id = ?1) THEN 'folder'
                     WHEN EXISTS(SELECT 1 FROM smart_folder_definition WHERE smart_folder_id = ?1)
                         THEN 'smart_folder'
                     WHEN EXISTS(SELECT 1 FROM media_item WHERE media_id = ?1) THEN 'item'
                     WHEN EXISTS(SELECT 1 FROM library_item WHERE local_id = ?1 AND item_kind = 2)
                         THEN 'collection'
                 END",
            )?
            .query_row([local_id], |row| row.get::<_, Option<String>>(0))?
            .and_then(|kind| SyncKind::parse(&kind)))
    }

    /// The sync key that the library gives a local entity now, or `None` when
    /// the entity is gone or its kind has no local id.
    pub(crate) fn key(&self, kind: SyncKind, local_id: u32) -> Result<Option<String>> {
        let sql = match kind {
            SyncKind::Item => {
                "SELECT file.content_hash FROM media_item media
                 JOIN media_file file ON file.file_id = media.file_id WHERE media.media_id = ?1"
            }
            SyncKind::Collection => {
                "SELECT stable_key FROM library_item WHERE local_id = ?1 AND item_kind = 2
                 AND EXISTS(SELECT 1 FROM library_root WHERE root_id = ?1)"
            }
            SyncKind::Tag => {
                "SELECT CASE WHEN namespace.display_name = '' THEN tag.subname
                             ELSE namespace.display_name || ':' || tag.subname END
                 FROM tag_definition tag
                 JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
                 WHERE tag.tag_id = ?1"
            }
            SyncKind::TagGroup => "SELECT display_name FROM tag_namespace WHERE namespace_id = ?1",
            SyncKind::Folder => "SELECT stable_key FROM folder_definition WHERE folder_id = ?1",
            SyncKind::SmartFolder => "SELECT stable_key FROM smart_folder_definition WHERE smart_folder_id = ?1",
            SyncKind::Library | SyncKind::ViewSettings | SyncKind::DuplicateDecision => return Ok(None),
        };
        Ok(self
            .connection
            .prepare_cached(sql)?
            .query_row([local_id], |row| row.get::<_, String>(0))
            .optional()?)
    }

    /// The sync key and kind of a root (an item outside a collection, or a
    /// collection).
    pub(crate) fn root_key(&self, root_id: RootId) -> Result<Option<(SyncKind, String)>> {
        let kind = if self.snapshot.collection_orders.contains_key(&root_id) {
            SyncKind::Collection
        } else {
            SyncKind::Item
        };
        Ok(self.key(kind, root_id.0)?.map(|key| (kind, key)))
    }

    /// The local id of an entity, found by its sync key in the library;
    /// `None` also for the kinds without a local id.
    pub(crate) fn find(&self, kind: SyncKind, key: &str) -> Result<Option<u32>> {
        if kind == SyncKind::Tag {
            return Ok(self.snapshot.tag_ids_by_name.get(key).map(|id| id.0));
        }
        let sql = match kind {
            SyncKind::Item => {
                "SELECT media.media_id FROM media_file file
                 JOIN media_item media ON media.file_id = file.file_id WHERE file.content_hash = ?1"
            }
            SyncKind::Collection => {
                "SELECT local_id FROM library_item WHERE stable_key = ?1 AND item_kind = 2"
            }
            SyncKind::TagGroup => "SELECT namespace_id FROM tag_namespace WHERE display_name = ?1",
            SyncKind::Folder => "SELECT folder_id FROM folder_definition WHERE stable_key = ?1",
            SyncKind::SmartFolder => "SELECT smart_folder_id FROM smart_folder_definition WHERE stable_key = ?1",
            SyncKind::Tag => unreachable!("tags are found in the projection"),
            SyncKind::Library | SyncKind::ViewSettings | SyncKind::DuplicateDecision => return Ok(None),
        };
        Ok(self
            .connection
            .prepare_cached(sql)?
            .query_row([key], |row| row.get::<_, u32>(0))
            .optional()?)
    }

    /// The state of a local entity in sync keys, or `None` when it is gone.
    pub(crate) fn view(&mut self, kind: SyncKind, local_id: u32) -> Result<Option<View>> {
        let mut view = View::default();
        match kind {
            SyncKind::Item => {
                let row = self
                    .connection
                    .prepare_cached(
                        "SELECT file.mime, file.size_bytes, file.width, file.height,
                                file.duration_ms, file.frame_count, media.media_name,
                                media.media_notes, root.root_id IS NOT NULL, root.name, root.notes,
                                root.source_urls_json, root.imported_at_ms, root.captured_at_ms,
                                root.modified_at_ms,
                                (SELECT viewed_at_ms FROM recent_view WHERE root_id = root.root_id)
                         FROM media_item media
                         JOIN media_file file ON file.file_id = media.file_id
                         LEFT JOIN library_root root ON root.root_id = media.media_id
                         WHERE media.media_id = ?1",
                    )?
                    .query_row([local_id], |row| {
                        let file = json!({
                            "mime": row.get::<_, String>(0)?,
                            "size": row.get::<_, i64>(1)?,
                            "width": row.get::<_, Option<i64>>(2)?,
                            "height": row.get::<_, Option<i64>>(3)?,
                            "duration_ms": row.get::<_, Option<i64>>(4)?,
                            "frames": row.get::<_, Option<i64>>(5)?,
                        });
                        let standalone = row.get::<_, bool>(8)?;
                        let (name, notes) = if standalone {
                            (row.get::<_, String>(9)?, row.get::<_, Option<String>>(10)?)
                        } else {
                            (row.get::<_, String>(6)?, row.get::<_, Option<String>>(7)?)
                        };
                        let root = standalone
                            .then(|| -> rusqlite::Result<_> {
                                Ok(RootRow {
                                    links: row.get(11)?,
                                    imported: row.get(12)?,
                                    captured: row.get(13)?,
                                    modified: row.get(14)?,
                                    viewed: row.get(15)?,
                                })
                            })
                            .transpose()?;
                        Ok((file, name, notes, root))
                    })
                    .optional()?;
                let Some((file, name, notes, root)) = row else {
                    return Ok(None);
                };
                view.fields.insert("file".into(), file);
                view.fields.insert("name".into(), json!(name));
                view.fields.insert("notes".into(), json!(notes));
                view.fields.insert("sources".into(), self.sources(local_id)?);
                match root {
                    Some(root) => self.root_fields(local_id, &root, &mut view)?,
                    None => {
                        let owner = self.snapshot.media_owner.get(local_id).copied();
                        view.owner = match owner {
                            Some(owner) => self.key(SyncKind::Collection, owner.0)?,
                            None => None,
                        };
                    }
                }
            }
            SyncKind::Collection => {
                let row = self
                    .connection
                    .prepare_cached(
                        "SELECT root.name, root.notes, file.content_hash, root.source_urls_json,
                                root.imported_at_ms, root.captured_at_ms, root.modified_at_ms,
                                (SELECT viewed_at_ms FROM recent_view WHERE root_id = root.root_id)
                         FROM library_item item
                         JOIN library_root root ON root.root_id = item.local_id
                         JOIN media_item media ON media.media_id = root.cover_media_id
                         JOIN media_file file ON file.file_id = media.file_id
                         WHERE item.local_id = ?1 AND item.item_kind = 2",
                    )?
                    .query_row([local_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            RootRow {
                                links: row.get(3)?,
                                imported: row.get(4)?,
                                captured: row.get(5)?,
                                modified: row.get(6)?,
                                viewed: row.get(7)?,
                            },
                        ))
                    })
                    .optional()?;
                let Some((name, notes, cover, root)) = row else {
                    return Ok(None);
                };
                view.fields.insert("name".into(), json!(name));
                view.fields.insert("notes".into(), json!(notes));
                view.fields.insert("cover".into(), json!(cover));
                self.root_fields(local_id, &root, &mut view)?;
            }
            SyncKind::Tag | SyncKind::TagGroup => {
                if self.key(kind, local_id)?.is_none() {
                    return Ok(None);
                }
            }
            SyncKind::Folder => {
                let row = self
                    .connection
                    .prepare_cached(
                        "SELECT name, icon, color, notes, auto_tag_ids, cover_root_id, watch_path,
                                watch_enabled, watch_subfolders, watch_lifecycle, parent_id
                         FROM folder_definition WHERE folder_id = ?1",
                    )?
                    .query_row([local_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Vec<u8>>(4)?,
                            row.get::<_, Option<u32>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, bool>(7)?,
                            row.get::<_, bool>(8)?,
                            row.get::<_, i64>(9)?,
                            row.get::<_, Option<u32>>(10)?,
                        ))
                    })
                    .optional()?;
                let Some((name, icon, color, notes, auto, cover, path, enabled, subfolders, lifecycle, parent)) =
                    row
                else {
                    return Ok(None);
                };
                let auto = if auto.is_empty() {
                    RoaringBitmap::new()
                } else {
                    RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(auto))?
                };
                let names = self.tag_names();
                let auto = auto
                    .iter()
                    .filter_map(|tag| names.get(&tag).cloned())
                    .collect::<BTreeSet<_>>();
                let cover = match cover {
                    Some(root) => match self.root_key(RootId(root))? {
                        Some((kind, key)) => json!([kind.as_str(), key]),
                        None => Value::Null,
                    },
                    None => Value::Null,
                };
                let watch = path.map_or(Value::Null, |path| {
                    json!({
                        "path": path,
                        "enabled": enabled,
                        "subfolders": subfolders,
                        "lifecycle": lifecycle_name(Lifecycle::from_watch_code(lifecycle)),
                    })
                });
                for (field, value) in [
                    ("name", json!(name)),
                    ("icon", json!(icon)),
                    ("color", json!(color)),
                    ("notes", json!(notes)),
                    ("auto_tags", json!(auto)),
                    ("cover", cover),
                    ("watch", watch),
                ] {
                    view.fields.insert(field.into(), value);
                }
                view.owner = match parent {
                    Some(parent) => self.folder_key(parent)?,
                    None => None,
                };
            }
            SyncKind::SmartFolder => {
                let row = self
                    .connection
                    .prepare_cached(
                        "SELECT name, icon, color, notes, view_query_json, parent_id
                         FROM smart_folder_definition WHERE smart_folder_id = ?1",
                    )?
                    .query_row([local_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<u32>>(5)?,
                        ))
                    })
                    .optional()?;
                let Some((name, icon, color, notes, rules, parent)) = row else {
                    return Ok(None);
                };
                let rules = self.rules(&serde_json::from_str(&rules)?)?;
                for (field, value) in [
                    ("name", json!(name)),
                    ("icon", json!(icon)),
                    ("color", json!(color)),
                    ("notes", json!(notes)),
                    ("rules", rules),
                ] {
                    view.fields.insert(field.into(), value);
                }
                view.owner = match parent {
                    Some(parent) => self.key(SyncKind::SmartFolder, parent)?,
                    None => None,
                };
            }
            SyncKind::Library | SyncKind::ViewSettings | SyncKind::DuplicateDecision => return Ok(None),
        }
        Ok(Some(view))
    }

    /// The rules of a smart folder in sync keys: the `tag_ids` of each tag
    /// rule become `tags`, the names of those tags, sorted, and the
    /// `folder_ids` of each folder rule become `folders`, the keys of those
    /// folders in their order, with `null` for a folder that the library does
    /// not have. Such a folder matches nothing; the projection writes it as
    /// the id 0, which no folder has.
    fn rules(&mut self, rules: &ViewQuerySpec) -> Result<Value> {
        let mut rules = serde_json::to_value(rules)?;
        self.rules_value(&mut rules)?;
        Ok(rules)
    }

    fn rules_value(&mut self, value: &mut Value) -> Result<()> {
        match value {
            Value::Object(object) => match object.get("clause").and_then(Value::as_str) {
                Some("tags") => {
                    let ids = object.remove("tag_ids").unwrap_or_default();
                    let names = self.tag_names();
                    let tags = ids
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|id| names.get(&(id.as_u64()? as u32)).cloned())
                        .collect::<BTreeSet<_>>();
                    object.insert("tags".into(), json!(tags));
                }
                Some("folders") => {
                    let ids = object.remove("folder_ids").unwrap_or_default();
                    let mut keys = Vec::new();
                    for id in ids.as_array().into_iter().flatten() {
                        let key = match id.as_u64() {
                            Some(id) => self.folder_key(id as u32)?,
                            None => None,
                        };
                        keys.push(json!(key));
                    }
                    object.insert("folders".into(), Value::Array(keys));
                }
                _ => {
                    for child in object.values_mut() {
                        self.rules_value(child)?;
                    }
                }
            },
            Value::Array(values) => {
                for child in values {
                    self.rules_value(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The sources of a media item: its rows of `source_provenance`, sorted.
    pub(crate) fn sources(&self, media_id: u32) -> Result<Value> {
        let mut statement = self.connection.prepare_cached(
            "SELECT source_key, source_item_key, source_text FROM source_provenance
             WHERE media_id = ?1 ORDER BY source_key, source_item_key",
        )?;
        let rows = statement.query_map([media_id], |row| {
            Ok(json!({
                "source_key": row.get::<_, String>(0)?,
                "source_item_key": row.get::<_, String>(1)?,
                "source_text": row.get::<_, Option<String>>(2)?,
            }))
        })?;
        Ok(Value::Array(rows.collect::<rusqlite::Result<Vec<_>>>()?))
    }

    /// The state of an entity of a kind without a local id, found by its
    /// sync key, or `None` when the library does not have it.
    pub(crate) fn row_view(&self, kind: SyncKind, key: &str) -> Result<Option<View>> {
        match kind {
            SyncKind::Library => self.settings_view().map(Some),
            SyncKind::ViewSettings => self.view_settings(key),
            SyncKind::DuplicateDecision => self.decision_view(key),
            _ => Ok(None),
        }
    }

    /// The sync key of a row that `changed_row` names by its kind and text,
    /// or `None` when the row names no entity that the library has: a view
    /// of a folder or smart folder that is gone, or a duplicate pair of a
    /// file that is gone.
    pub(crate) fn row_key(&self, kind: SyncKind, row: &str) -> Result<Option<String>> {
        match kind {
            SyncKind::Library => Ok(Some(String::new())),
            SyncKind::ViewSettings => self.scope_key(row),
            SyncKind::DuplicateDecision => {
                let Some((a, b)) = row.split_once(':') else { return Ok(None) };
                let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else { return Ok(None) };
                let hash = |file_id: u32| -> Result<Option<String>> {
                    Ok(self
                        .connection
                        .prepare_cached("SELECT content_hash FROM media_file WHERE file_id = ?1")?
                        .query_row([file_id], |row| row.get::<_, String>(0))
                        .optional()?)
                };
                Ok(hash(a)?.zip(hash(b)?).map(|(a, b)| super::decision_key(&a, &b)))
            }
            _ => Ok(None),
        }
    }

    /// The keys of the "Not duplicate" decisions about the file of the item
    /// `media_id`.
    pub(crate) fn decisions_of(&self, media_id: u32) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT a.content_hash, b.content_hash FROM media_item media
             JOIN duplicate_pair pair ON pair.file_id_a = media.file_id OR pair.file_id_b = media.file_id
             JOIN media_file a ON a.file_id = pair.file_id_a
             JOIN media_file b ON b.file_id = pair.file_id_b
             WHERE media.media_id = ?1 AND pair.status = 2",
        )?;
        let rows = statement.query_map([media_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut keys = Vec::new();
        for row in rows {
            let (a, b) = row?;
            keys.push(super::decision_key(&a, &b));
        }
        Ok(keys)
    }

    /// The sync keys of every entity of the kinds without a local id that
    /// the library has.
    pub(crate) fn all_rows(&self) -> Result<Vec<(SyncKind, String)>> {
        let mut rows = vec![(SyncKind::Library, String::new())];
        let scopes = self
            .connection
            .prepare("SELECT scope FROM view_pref")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for scope in scopes {
            rows.extend(self.scope_key(&scope)?.map(|key| (SyncKind::ViewSettings, key)));
        }
        let pairs = self
            .connection
            .prepare("SELECT file_id_a || ':' || file_id_b FROM duplicate_pair WHERE status = 2")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for pair in pairs {
            let key = self.row_key(SyncKind::DuplicateDecision, &pair)?;
            rows.extend(key.map(|key| (SyncKind::DuplicateDecision, key)));
        }
        Ok(rows)
    }

    /// The sync key of a scope of `view_pref`, or of an entry of the Quick
    /// Access list, which names views the same way: `folder:<id>` and
    /// `smart:<id>` become `folder:<uuid>` and `smart:<uuid>`, and any other
    /// text stays as it is. `None` when the folder or smart folder is gone.
    pub(crate) fn scope_key(&self, scope: &str) -> Result<Option<String>> {
        let (prefix, kind, id) = if let Some(id) = scope.strip_prefix("folder:") {
            ("folder:", SyncKind::Folder, id)
        } else if let Some(id) = scope.strip_prefix("smart:") {
            ("smart:", SyncKind::SmartFolder, id)
        } else {
            return Ok(Some(scope.to_owned()));
        };
        let Ok(id) = id.parse::<u32>() else { return Ok(None) };
        Ok(self.key(kind, id)?.map(|key| format!("{prefix}{key}")))
    }

    /// The scope in the library of a sync key that `scope_key` made, or
    /// `None` when the library does not have the folder or smart folder.
    pub(crate) fn local_scope(&self, key: &str) -> Result<Option<String>> {
        match super::view_owner(key) {
            Some((kind, owner)) => {
                let prefix = if kind == SyncKind::Folder { "folder:" } else { "smart:" };
                Ok(self.find(kind, owner)?.map(|id| format!("{prefix}{id}")))
            }
            None => Ok(Some(key.to_owned())),
        }
    }

    /// The library's settings: one field `setting:<name>` for each field of
    /// the `application` setting that is not `null` and does not stay on
    /// this computer (`local_setting`). The Quick Access list names folders
    /// and smart folders by sync key, and leaves out those that are gone.
    fn settings_view(&self) -> Result<View> {
        let stored = self
            .connection
            .prepare_cached("SELECT value_json FROM setting WHERE key = 'application'")?
            .query_row([], |row| row.get::<_, String>(0))
            .optional()?;
        let settings = stored
            .and_then(|stored| serde_json::from_str::<Value>(&stored).ok())
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let mut view = View::default();
        for (name, value) in settings {
            if local_setting(&name) || value.is_null() {
                continue;
            }
            let value = match (name.as_str(), value) {
                (QUICK_ACCESS, Value::Array(entries)) => {
                    let mut keys = Vec::with_capacity(entries.len());
                    for entry in entries {
                        match entry.as_str() {
                            Some(scope) => keys.extend(self.scope_key(scope)?.map(Value::String)),
                            None => keys.push(entry),
                        }
                    }
                    Value::Array(keys)
                }
                (_, value) => value,
            };
            view.fields.insert(format!("setting:{name}"), value);
        }
        Ok(view)
    }

    /// The settings of the view with the sync key `key`: the field `value`
    /// with the row's JSON. `None` when the view has no row or its folder or
    /// smart folder is gone.
    fn view_settings(&self, key: &str) -> Result<Option<View>> {
        let Some(scope) = self.local_scope(key)? else { return Ok(None) };
        let stored = self
            .connection
            .prepare_cached("SELECT value_json FROM view_pref WHERE scope = ?1")?
            .query_row([&scope], |row| row.get::<_, String>(0))
            .optional()?;
        Ok(stored.map(|stored| {
            let mut view = View::default();
            let value = serde_json::from_str::<Value>(&stored).unwrap_or(Value::String(stored));
            view.fields.insert("value".into(), value);
            view
        }))
    }

    /// The "Not duplicate" decision with the sync key `key`: the field
    /// `decision`, while the pair of the two files has status 2 and both
    /// files are items of the library.
    fn decision_view(&self, key: &str) -> Result<Option<View>> {
        let Some((a, b)) = key.split_once(':') else { return Ok(None) };
        let decided = self
            .connection
            .prepare_cached(
                "SELECT EXISTS(SELECT 1 FROM media_file a, media_file b, duplicate_pair pair
                 WHERE a.content_hash = ?1 AND b.content_hash = ?2
                   AND pair.file_id_a = MIN(a.file_id, b.file_id) AND pair.file_id_b = MAX(a.file_id, b.file_id)
                   AND pair.status = 2
                   AND EXISTS(SELECT 1 FROM media_item WHERE file_id = a.file_id)
                   AND EXISTS(SELECT 1 FROM media_item WHERE file_id = b.file_id))",
            )?
            .query_row([a, b], |row| row.get::<_, bool>(0))?;
        Ok(decided.then(|| {
            let mut view = View::default();
            view.fields.insert("decision".into(), json!(KEEP_BOTH));
            view
        }))
    }

    fn root_fields(&mut self, root_id: u32, root: &RootRow, view: &mut View) -> Result<()> {
        self.prepare(&RoaringBitmap::from_iter([root_id]))?;
        let lifecycle = self
            .snapshot
            .lifecycle
            .iter()
            .find(|(_, members)| members.contains(root_id))
            .map_or(Lifecycle::Inbox, |(lifecycle, _)| *lifecycle);
        let rating = self
            .snapshot
            .ratings
            .iter()
            .find(|(_, members)| members.contains(root_id))
            .map_or(0, |(rating, _)| rating.bitmap_key());
        let links = serde_json::from_str::<Value>(&root.links).unwrap_or_else(|_| json!([]));
        for (field, value) in [
            ("links", links),
            ("imported_at", json!(root.imported)),
            ("captured_at", json!(root.captured)),
            ("modified_at", json!(root.modified)),
            ("lifecycle", json!(lifecycle_name(lifecycle))),
            ("rating", json!(rating)),
            ("viewed_at", json!(root.viewed)),
        ] {
            view.fields.insert(field.into(), value);
        }
        view.tags = self.root_tags.get(&root_id).into_iter().flatten().cloned().collect();
        view.folders = self.root_folders.get(&root_id).into_iter().flatten().cloned().collect();
        Ok(())
    }
}

/// The columns of `library_root` that the root fields of an item or
/// collection come from, and the time it was last viewed (`recent_view`).
struct RootRow {
    links: String,
    imported: i64,
    captured: Option<i64>,
    modified: i64,
    viewed: Option<i64>,
}
