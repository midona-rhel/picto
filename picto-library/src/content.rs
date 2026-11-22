//! Reads everything a library holds through a connection that only reads,
//! so that the content can be merged into another library.
//!
//! The functions here take a plain SQLite connection and never write. The
//! caller opens the connection read-only and checks the schema with
//! [`crate::schema::validate`] first. A library's tags, ratings and
//! lifecycles are stored as one bitmap of roots for each value, and its
//! folders as one ordered list of roots for each folder. [`for_each_root`]
//! reads those once and turns them round, so that reading every root costs
//! one pass over the rows and not one search of every bitmap for each root.

use std::collections::HashMap;

use roaring::RoaringBitmap;
use rusqlite::{params, Connection};

use crate::bitmap::{self, BitmapDomain};
use crate::model::{
    FileId, FolderId, ImmutableMediaFacts, Lifecycle, MediaId, MediaRecord, Rating, RootDetails,
    RootId, RootKind, RootRecord, SmartFolderId, TagId,
};
use crate::ordering::{self, OrderOwnerKind};
use crate::predicate::ViewQuerySpec;
use crate::{LibraryError, Result};

/// A tag of the library: its id there and its full name, which is
/// `namespace:name`, or the name alone for a tag without a namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTag {
    pub tag_id: TagId,
    pub name: String,
}

/// A folder of the library, without the roots in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentFolder {
    pub folder_id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    /// The tags that the folder gives every item put into it.
    pub auto_tag_ids: Vec<TagId>,
    pub display_order: i64,
}

/// A smart folder of the library. The tag ids and folder ids in `view` are
/// this library's ids.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentSmartFolder {
    pub smart_folder_id: SmartFolderId,
    pub parent_id: Option<SmartFolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub view: ViewQuerySpec,
    pub display_order: i64,
}

/// What a library holds besides its items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outline {
    /// The number of items: files that stand alone, and collections.
    pub items: u64,
    /// The number of files, counting each member of a collection.
    pub files: u64,
    pub tags: Vec<ContentTag>,
    /// The folders, each parent before its children.
    pub folders: Vec<ContentFolder>,
    /// The smart folders, each parent before its children.
    pub smart_folders: Vec<ContentSmartFolder>,
}

/// Returns how many items and how many files the library holds.
pub fn counts(connection: &Connection) -> Result<(u64, u64)> {
    let items = connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let files = connection.query_row("SELECT COUNT(*) FROM media_item", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok((items.max(0) as u64, files.max(0) as u64))
}

/// Reads the library's tags, folders and smart folders, and its counts.
pub fn outline(connection: &Connection) -> Result<Outline> {
    let (items, files) = counts(connection)?;
    let mut statement = connection.prepare(
        "SELECT tag.tag_id, namespace.display_name, tag.subname
         FROM tag_definition tag
         JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
         ORDER BY tag.tag_id",
    )?;
    let tags = statement
        .query_map([], |row| {
            let namespace = row.get::<_, String>(1)?;
            let subname = row.get::<_, String>(2)?;
            Ok(ContentTag {
                tag_id: TagId(row.get(0)?),
                name: if namespace.is_empty() {
                    subname
                } else {
                    format!("{namespace}:{subname}")
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut statement = connection.prepare(
        "SELECT folder_id, parent_id, name, icon, color, notes, auto_tag_ids, display_order
         FROM folder_definition
         ORDER BY display_order, folder_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                ContentFolder {
                    folder_id: FolderId(row.get(0)?),
                    parent_id: row.get::<_, Option<u32>>(1)?.map(FolderId),
                    name: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    auto_tag_ids: Vec::new(),
                    display_order: row.get(7)?,
                },
                row.get::<_, Vec<u8>>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut folders = Vec::with_capacity(rows.len());
    for (mut folder, auto_tags) in rows {
        if !auto_tags.is_empty() {
            folder.auto_tag_ids =
                RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(auto_tags))?
                    .into_iter()
                    .map(TagId)
                    .collect();
        }
        folders.push(folder);
    }
    let folders = parents_first(folders, |folder| folder.folder_id.0, |folder| {
        folder.parent_id.map(|id| id.0)
    });

    let mut statement = connection.prepare(
        "SELECT smart_folder_id, parent_id, name, icon, color, notes, view_query_json,
                display_order
         FROM smart_folder_definition
         ORDER BY display_order, smart_folder_id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, Option<u32>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut smart_folders = Vec::with_capacity(rows.len());
    for (id, parent, name, icon, color, notes, view, display_order) in rows {
        smart_folders.push(ContentSmartFolder {
            smart_folder_id: SmartFolderId(id),
            parent_id: parent.map(SmartFolderId),
            name,
            icon,
            color,
            notes,
            view: serde_json::from_str(&view)?,
            display_order,
        });
    }
    let smart_folders = parents_first(
        smart_folders,
        |folder| folder.smart_folder_id.0,
        |folder| folder.parent_id.map(|id| id.0),
    );

    Ok(Outline {
        items,
        files,
        tags,
        folders,
        smart_folders,
    })
}

/// Orders the rows of a tree so that each parent comes before its children.
/// Rows with the same parent keep their order. A row whose parent is not
/// among the rows is left out, together with its descendants.
fn parents_first<T>(
    rows: Vec<T>,
    id: impl Fn(&T) -> u32,
    parent: impl Fn(&T) -> Option<u32>,
) -> Vec<T> {
    let mut children = HashMap::<Option<u32>, Vec<T>>::new();
    for row in rows {
        children.entry(parent(&row)).or_default().push(row);
    }
    let mut ordered = Vec::new();
    let mut level = children.remove(&None).unwrap_or_default();
    while !level.is_empty() {
        let mut next = Vec::new();
        for row in level {
            if let Some(found) = children.remove(&Some(id(&row))) {
                next.extend(found);
            }
            ordered.push(row);
        }
        level = next;
    }
    ordered
}

/// Calls `each` with every item of the library, in the order of the items'
/// ids: its name, notes, dates and source links, its status and rating, the
/// ids of its tags and folders, and its files with their stored facts.
/// `each` returns `false` to stop the reading early.
pub fn for_each_root(
    connection: &Connection,
    mut each: impl FnMut(RootDetails) -> Result<bool>,
) -> Result<()> {
    let revision = crate::schema::validate(connection)?;
    let mut lifecycles = Vec::new();
    let mut ratings = Vec::new();
    let mut tags_of = HashMap::<u32, Vec<TagId>>::new();
    let mut bitmaps = bitmap::load_all(connection)?
        .into_iter()
        .collect::<Vec<_>>();
    bitmaps.sort_unstable_by_key(|(key, _)| (key.domain as u8, key.key_id));
    for (key, roots) in bitmaps {
        match key.domain {
            BitmapDomain::Lifecycle => {
                if let Some(value) = Lifecycle::ALL
                    .into_iter()
                    .find(|value| value.bitmap_key() == key.key_id)
                {
                    lifecycles.push((value, roots));
                }
            }
            BitmapDomain::Rating => {
                if let Some(value) = Rating::ALL
                    .into_iter()
                    .find(|value| value.bitmap_key() == key.key_id)
                {
                    ratings.push((value, roots));
                }
            }
            BitmapDomain::Tag => {
                for root in roots {
                    tags_of.entry(root).or_default().push(TagId(key.key_id));
                }
            }
        }
    }
    let mut folders_of = HashMap::<u32, Vec<FolderId>>::new();
    let mut members_of = HashMap::<u32, Vec<u32>>::new();
    ordering::load_all(connection, |kind, owner, values| {
        match kind {
            OrderOwnerKind::Folder => {
                for root in values {
                    folders_of.entry(root).or_default().push(FolderId(owner));
                }
            }
            OrderOwnerKind::Collection => {
                members_of.insert(owner, values);
            }
        }
        Ok(())
    })?;

    let mut member = connection.prepare(&format!(
        "SELECT {MEDIA_COLUMNS}
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id = ?1"
    ))?;
    // A file that stands alone has the same id as its root, so its row is
    // joined here. A collection has no such row; its members are read by id.
    let mut roots = connection.prepare(&format!(
        "SELECT root.root_id, item.stable_key, item.item_kind, root.name, root.notes,
                root.source_urls_json, root.cover_media_id, root.imported_at_ms,
                root.captured_at_ms, root.modified_at_ms, root.media_count,
                root.total_size_bytes, {MEDIA_COLUMNS}
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         LEFT JOIN media_item media ON media.media_id = root.root_id AND item.item_kind = 1
         LEFT JOIN media_file file ON file.file_id = media.file_id
         ORDER BY root.root_id"
    ))?;
    let mut rows = roots.query([])?;
    while let Some(row) = rows.next()? {
        let root_id = RootId(row.get(0)?);
        let kind = match row.get::<_, i64>(2)? {
            1 => RootKind::Media,
            2 => RootKind::Collection,
            value => {
                return Err(LibraryError::InvalidState(format!(
                    "root {root_id} has the unknown kind {value}"
                )))
            }
        };
        let root = RootRecord {
            root_id,
            stable_key: row.get(1)?,
            kind,
            name: row.get(3)?,
            notes: row.get(4)?,
            source_urls: serde_json::from_str(&row.get::<_, String>(5)?)?,
            cover_media_id: MediaId(row.get(6)?),
            imported_at_ms: row.get(7)?,
            captured_at_ms: row.get(8)?,
            modified_at_ms: row.get(9)?,
            media_count: row.get(10)?,
            total_size_bytes: row.get::<_, i64>(11)?.max(0) as u64,
        };
        let media = match kind {
            RootKind::Media => vec![media_record(row, 12)?],
            RootKind::Collection => {
                let ids = members_of.get(&root_id.0).ok_or_else(|| {
                    LibraryError::InvalidState(format!("collection {root_id} has no member order"))
                })?;
                let mut media = Vec::with_capacity(ids.len());
                for id in ids {
                    let mut found = member.query(params![id])?;
                    let row = found.next()?.ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "collection {root_id} names the missing file {id}"
                        ))
                    })?;
                    media.push(media_record(row, 0)?);
                }
                media
            }
        };
        let lifecycle = lifecycles
            .iter()
            .find_map(|(value, roots)| roots.contains(root_id.0).then_some(*value))
            .ok_or_else(|| LibraryError::InvalidState(format!("root {root_id} has no status")))?;
        let rating = ratings
            .iter()
            .find_map(|(value, roots)| roots.contains(root_id.0).then_some(*value))
            .unwrap_or(Rating::Unrated);
        let details = RootDetails {
            root,
            lifecycle,
            rating,
            folder_ids: folders_of.remove(&root_id.0).unwrap_or_default(),
            tag_ids: tags_of.remove(&root_id.0).unwrap_or_default(),
            media,
            revision,
        };
        if !each(details)? {
            break;
        }
    }
    Ok(())
}

/// The columns of one file, as [`media_record`] reads them.
const MEDIA_COLUMNS: &str = "media.media_id, media.media_name, media.media_notes, file.file_id,
     file.file_path, file.mime, file.size_bytes, file.width, file.height, file.duration_ms,
     file.frame_count, file.content_hash, file.perceptual_hash, file.palette_json";

/// Reads one file from the columns of [`MEDIA_COLUMNS`], the first of which
/// is at the index `at`.
fn media_record(row: &rusqlite::Row<'_>, at: usize) -> Result<MediaRecord> {
    Ok(MediaRecord {
        media_id: MediaId(row.get(at)?),
        media_name: row.get(at + 1)?,
        media_notes: row.get(at + 2)?,
        file_id: FileId(row.get(at + 3)?),
        file_path: row.get(at + 4)?,
        facts: ImmutableMediaFacts {
            mime: row.get(at + 5)?,
            size_bytes: row.get::<_, i64>(at + 6)?.max(0) as u64,
            width: row.get(at + 7)?,
            height: row.get(at + 8)?,
            duration_ms: row
                .get::<_, Option<i64>>(at + 9)?
                .map(|value| value.max(0) as u64),
            frame_count: row.get(at + 10)?,
            content_hash: row.get(at + 11)?,
            perceptual_hash: row.get(at + 12)?,
            palette: serde_json::from_str(&row.get::<_, String>(at + 13)?)?,
        },
    })
}
