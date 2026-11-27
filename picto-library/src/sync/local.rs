//! What the library holds, in sync keys: the key and the state of each local
//! entity, in the same form as `values::View`.

use std::collections::{BTreeSet, HashMap};

use roaring::RoaringBitmap;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use super::values::View;
use super::SyncKind;
use crate::model::{Lifecycle, Rating, RootId};
use crate::projection::ProjectionSnapshot;
use crate::Result;

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
                     WHEN EXISTS(SELECT 1 FROM media_item WHERE media_id = ?1) THEN 'item'
                     WHEN EXISTS(SELECT 1 FROM library_item WHERE local_id = ?1 AND item_kind = 2)
                         THEN 'collection'
                 END",
            )?
            .query_row([local_id], |row| row.get::<_, Option<String>>(0))?
            .and_then(|kind| SyncKind::parse(&kind)))
    }

    /// The sync key that the library gives a local entity now, or `None` when
    /// the entity is gone.
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

    /// The local id of an entity, found by its sync key in the library.
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
            SyncKind::Tag => unreachable!("tags are found in the projection"),
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
                                root.modified_at_ms
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
                                Ok((
                                    row.get::<_, String>(11)?,
                                    row.get::<_, i64>(12)?,
                                    row.get::<_, Option<i64>>(13)?,
                                    row.get::<_, i64>(14)?,
                                ))
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
                match root {
                    Some((links, imported, captured, modified)) => {
                        self.root_fields(local_id, &links, imported, captured, modified, &mut view)?;
                    }
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
                        "SELECT root.name, root.notes, root.source_urls_json, root.imported_at_ms,
                                root.captured_at_ms, root.modified_at_ms, file.content_hash
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
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    })
                    .optional()?;
                let Some((name, notes, links, imported, captured, modified, cover)) = row else {
                    return Ok(None);
                };
                view.fields.insert("name".into(), json!(name));
                view.fields.insert("notes".into(), json!(notes));
                view.fields.insert("cover".into(), json!(cover));
                self.root_fields(local_id, &links, imported, captured, modified, &mut view)?;
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
        }
        Ok(Some(view))
    }

    fn root_fields(
        &mut self,
        root_id: u32,
        links: &str,
        imported: i64,
        captured: Option<i64>,
        modified: i64,
        view: &mut View,
    ) -> Result<()> {
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
        let links = serde_json::from_str::<Value>(links).unwrap_or_else(|_| json!([]));
        for (field, value) in [
            ("links", links),
            ("imported_at", json!(imported)),
            ("captured_at", json!(captured)),
            ("modified_at", json!(modified)),
            ("lifecycle", json!(lifecycle_name(lifecycle))),
            ("rating", json!(rating)),
        ] {
            view.fields.insert(field.into(), value);
        }
        view.tags = self.root_tags.get(&root_id).into_iter().flatten().cloned().collect();
        view.folders = self.root_folders.get(&root_id).into_iter().flatten().cloned().collect();
        Ok(())
    }
}
