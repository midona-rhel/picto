//! Canonical read path for library item scopes.
//!
//! The query starts from `library_root` and projects collection members into
//! their owning collection. Every scope, filter, count, and page therefore
//! uses the same root set.

use rusqlite::{Connection, ToSql};
use serde::{Deserialize, Serialize};

use crate::app::{
    FileHash, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemTarget, Lifecycle,
};
use crate::store::Store;

const DEFAULT_PAGE_LIMIT: i64 = 100;
const MAX_PAGE_LIMIT: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemPageRequest {
    pub offset: i64,
    pub limit: i64,
}

impl Default for ItemPageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

impl ItemPageRequest {
    pub fn new(offset: i64, limit: i64) -> Self {
        Self { offset, limit }
    }

    fn normalized(self) -> Self {
        Self {
            offset: self.offset.max(0),
            limit: self.limit.clamp(1, MAX_PAGE_LIMIT),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub lifecycle: Lifecycle,
    pub label: Option<String>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub file_hash: Option<FileHash>,
    pub size_bytes: i64,
    pub rating: Option<i64>,
    pub captured_at: Option<String>,
    pub imported_at: Option<String>,
    pub cover_media_item_id: Option<ItemId>,
    pub cover_mime_type: Option<String>,
    pub cover_file_hash: Option<FileHash>,
    pub cover_size_bytes: Option<i64>,
    pub collection_member_count: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemPage {
    pub items: Vec<ItemSummary>,
    pub revision: u64,
    pub visible_item_count: i64,
    pub visible_media_count: i64,
}

/// Resolve one canonical page from the replacement store.
pub fn query(
    store: &Store,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> Result<ItemPage, String> {
    let page = page.normalized();
    store.read(|connection| resolve_connection(connection, item_query, page))
}

/// Resolve a mutation target inside the caller's transaction. Query targets
/// use the same scope and filter compiler as grid pages and counts.
pub(crate) fn resolve_target_ids(
    connection: &Connection,
    target: &ItemTarget,
) -> rusqlite::Result<Vec<i64>> {
    match target {
        ItemTarget::Explicit { item_ids } => {
            let ids = item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<std::collections::BTreeSet<_>>();
            if ids.is_empty() || ids.len() != item_ids.len() {
                return Err(invalid_target(
                    "An explicit target must contain unique library root IDs",
                ));
            }
            let mut arguments: Vec<Box<dyn ToSql>> = Vec::with_capacity(ids.len());
            let placeholders = ids
                .iter()
                .map(|item_id| {
                    let index = push_argument(&mut arguments, *item_id);
                    format!("?{index}")
                })
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT item_id FROM library_root
                 WHERE item_id IN ({placeholders}) ORDER BY item_id"
            );
            let references: Vec<&dyn ToSql> =
                arguments.iter().map(|value| value.as_ref()).collect();
            let mut statement = connection.prepare(&sql)?;
            let resolved = statement
                .query_map(references.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if resolved.len() != ids.len() {
                return Err(invalid_target("A targeted item is not a library root"));
            }
            Ok(resolved)
        }
        ItemTarget::Query {
            query,
            excluded_item_ids,
        } => {
            let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
                ItemScope::Folder { folder_id } => *folder_id,
                _ => -1,
            })];
            let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
            apply_filters(&query.filters, &mut predicates, &mut arguments);
            if !excluded_item_ids.is_empty() {
                let excluded = excluded_item_ids
                    .iter()
                    .map(|item_id| {
                        let index = push_argument(&mut arguments, item_id.0);
                        format!("?{index}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                predicates.push(format!("ri.item_id NOT IN ({excluded})"));
            }
            let sql = format!(
                "WITH
                 root_items AS (
                     SELECT lr.item_id, lr.lifecycle, li.kind, li.label,
                            li.cover_media_item_id, li.created_at, li.updated_at,
                            fi.position_rank AS folder_position, mv.viewed_at
                     FROM library_root lr
                     JOIN library_item li ON li.item_id = lr.item_id
                     LEFT JOIN folder_item fi
                       ON fi.item_id = lr.item_id AND fi.folder_id = ?1
                     LEFT JOIN media_view mv ON mv.item_id = lr.item_id
                     WHERE li.kind = 'collection'
                        OR NOT EXISTS (
                            SELECT 1 FROM collection_member member_root
                            WHERE member_root.media_item_id = lr.item_id
                        )
                 ),
                 root_media AS (
                     SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
                     FROM root_items ri WHERE ri.kind = 'media'
                     UNION ALL
                     SELECT ri.item_id, cm.media_item_id
                     FROM root_items ri
                     JOIN collection_member cm ON cm.collection_id = ri.item_id
                     WHERE ri.kind = 'collection'
                 )
                 SELECT ri.item_id FROM root_items ri
                 WHERE {}
                 ORDER BY ri.item_id",
                predicates.join(" AND ")
            );
            let references: Vec<&dyn ToSql> =
                arguments.iter().map(|value| value.as_ref()).collect();
            let mut statement = connection.prepare(&sql)?;
            let resolved = statement
                .query_map(references.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(resolved)
        }
    }
}

fn invalid_target(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn resolve_connection(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> rusqlite::Result<ItemPage> {
    let mut arguments: Vec<Box<dyn ToSql>> = Vec::new();

    // Parameter 1 is always present so the root CTE can expose folder order
    // without requiring a separate SQL shape for folder queries.
    arguments.push(Box::new(match &item_query.scope {
        ItemScope::Folder { folder_id } => *folder_id,
        _ => -1,
    }));

    let mut predicates = vec![scope_predicate(
        connection,
        &item_query.scope,
        &mut arguments,
    )?];
    apply_filters(&item_query.filters, &mut predicates, &mut arguments);
    let where_clause = predicates.join(" AND ");

    let order_by = order_expression(item_query, &mut arguments);
    let limit_index = arguments.len() + 1;
    arguments.push(Box::new(page.limit));
    let offset_index = arguments.len() + 1;
    arguments.push(Box::new(page.offset));

    let sql = format!(
        "WITH
         root_items AS (
             SELECT
                 lr.item_id,
                 lr.lifecycle,
                 li.kind,
                 li.label,
                 li.cover_media_item_id,
                 li.created_at,
                 li.updated_at,
                 fi.position_rank AS folder_position,
                 mv.viewed_at
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             LEFT JOIN folder_item fi
               ON fi.item_id = lr.item_id AND fi.folder_id = ?1
             LEFT JOIN media_view mv ON mv.item_id = lr.item_id
             WHERE li.kind = 'collection'
                OR NOT EXISTS (
                    SELECT 1
                    FROM collection_member member_root
                    WHERE member_root.media_item_id = lr.item_id
                )
         ),
         root_media AS (
             SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
             FROM root_items ri
             WHERE ri.kind = 'media'
             UNION ALL
             SELECT ri.item_id, cm.media_item_id
             FROM root_items ri
             JOIN collection_member cm ON cm.collection_id = ri.item_id
             WHERE ri.kind = 'collection'
         ),
         filtered_roots AS (
             SELECT
                 ri.item_id,
                 ri.lifecycle,
                 ri.kind,
                 ri.label,
                 ri.cover_media_item_id,
                 ri.created_at,
                 ri.updated_at,
                 ri.folder_position,
                 ri.viewed_at,
                 (
                     SELECT COUNT(*)
                     FROM root_media rm
                     WHERE rm.root_item_id = ri.item_id
                 ) AS collection_member_count,
                 COALESCE((
                     SELECT SUM(mf.size_bytes)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     WHERE rm.root_item_id = ri.item_id
                 ), 0) AS total_size_bytes,
                 COALESCE((
                     SELECT MAX(ma.imported_at)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ), ri.created_at) AS imported_at,
                 (
                     SELECT MAX(ma.captured_at)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ) AS captured_at,
                 COALESCE(ri.label, (
                     SELECT ma.name
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                     ORDER BY rm.media_item_id ASC
                     LIMIT 1
                 )) AS sort_name,
                 (
                     SELECT MAX(ma.rating)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ) AS sort_rating,
                 CASE
                     WHEN ri.kind = 'collection' THEN COALESCE(
                         ri.cover_media_item_id,
                         (
                             SELECT cm.media_item_id
                             FROM collection_member cm
                             WHERE cm.collection_id = ri.item_id
                             ORDER BY cm.position_rank ASC, cm.media_item_id ASC
                             LIMIT 1
                         )
                     )
                     ELSE ri.item_id
                 END AS resolved_cover_media_item_id
             FROM root_items ri
             WHERE {where_clause}
         ),
         metrics AS (
             SELECT
                 (SELECT revision FROM library_meta WHERE singleton = 1) AS revision,
                 COUNT(*) AS visible_item_count,
                 COALESCE(SUM(collection_member_count), 0) AS visible_media_count
             FROM filtered_roots
         ),
         paged AS (
             SELECT
                 fr.*,
                 root_asset.name AS root_name,
                 root_file.mime_type AS root_mime_type,
                 root_file.file_hash AS root_file_hash,
                 root_file.size_bytes AS root_size_bytes,
                 root_asset.rating AS root_rating,
                 cover_file.mime_type AS cover_mime_type,
                 cover_file.file_hash AS cover_file_hash,
                 cover_file.size_bytes AS cover_size_bytes
             FROM filtered_roots fr
             LEFT JOIN media_asset root_asset
               ON root_asset.item_id = fr.item_id AND fr.kind = 'media'
             LEFT JOIN media_file root_file ON root_file.file_id = root_asset.file_id
             LEFT JOIN media_asset cover_asset
               ON cover_asset.item_id = fr.resolved_cover_media_item_id
             LEFT JOIN media_file cover_file ON cover_file.file_id = cover_asset.file_id
             ORDER BY {order_by}, fr.item_id ASC
             LIMIT ?{limit_index} OFFSET ?{offset_index}
         )
         SELECT
             metrics.revision,
             metrics.visible_item_count,
             metrics.visible_media_count,
             paged.item_id,
             paged.kind,
             paged.lifecycle,
             paged.label,
             paged.root_name,
             paged.root_mime_type,
             paged.root_file_hash,
             paged.root_size_bytes,
             paged.root_rating,
             paged.captured_at,
             paged.imported_at,
             paged.resolved_cover_media_item_id,
             paged.cover_mime_type,
             paged.cover_file_hash,
             paged.cover_size_bytes,
             paged.collection_member_count,
             paged.total_size_bytes
         FROM metrics
         LEFT JOIN paged ON TRUE"
    );

    let references: Vec<&dyn ToSql> = arguments.iter().map(|value| value.as_ref()).collect();
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(references.as_slice())?;

    let mut page_result: Option<ItemPage> = None;
    while let Some(row) = rows.next()? {
        let revision: u64 = row.get(0)?;
        let visible_item_count: i64 = row.get(1)?;
        let visible_media_count: i64 = row.get(2)?;
        let item_id: Option<i64> = row.get(3)?;
        let items = if let Some(item_id) = item_id {
            vec![read_summary(row, item_id)?]
        } else {
            Vec::new()
        };

        if let Some(result) = &mut page_result {
            result.items.extend(items);
        } else {
            page_result = Some(ItemPage {
                items,
                revision,
                visible_item_count,
                visible_media_count,
            });
        }
    }

    Ok(page_result.unwrap_or(ItemPage {
        items: Vec::new(),
        revision: 0,
        visible_item_count: 0,
        visible_media_count: 0,
    }))
}

fn read_summary(row: &rusqlite::Row<'_>, item_id: i64) -> rusqlite::Result<ItemSummary> {
    let kind = match row.get::<_, String>(4)?.as_str() {
        "media" => ItemKind::Media,
        "collection" => ItemKind::Collection,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                4,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };
    let lifecycle = match row.get::<_, String>(5)?.as_str() {
        "inbox" => Lifecycle::Inbox,
        "active" => Lifecycle::Active,
        "trash" => Lifecycle::Trash,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                5,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };

    let root_size_bytes: Option<i64> = row.get(10)?;
    let total_size_bytes: i64 = row.get(19)?;
    Ok(ItemSummary {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        label: row.get(6)?,
        name: row.get(7)?,
        mime_type: row.get(8)?,
        file_hash: row.get::<_, Option<String>>(9)?.map(FileHash),
        size_bytes: root_size_bytes.unwrap_or(total_size_bytes),
        rating: row.get(11)?,
        captured_at: row.get(12)?,
        imported_at: row.get(13)?,
        cover_media_item_id: row.get::<_, Option<i64>>(14)?.map(ItemId),
        cover_mime_type: row.get(15)?,
        cover_file_hash: row.get::<_, Option<String>>(16)?.map(FileHash),
        cover_size_bytes: row.get(17)?,
        collection_member_count: row.get(18)?,
        total_size_bytes,
    })
}

fn scope_predicate(
    connection: &Connection,
    scope: &ItemScope,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let predicate = match scope {
        ItemScope::All => "ri.lifecycle = 'active'".to_string(),
        ItemScope::Inbox => "ri.lifecycle = 'inbox'".to_string(),
        ItemScope::Trash => "ri.lifecycle = 'trash'".to_string(),
        ItemScope::RecentlyViewed => {
            "ri.lifecycle = 'active' AND ri.viewed_at IS NOT NULL".to_string()
        }
        ItemScope::Untagged => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Uncategorized => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1 FROM folder_item categorized_folder
                 WHERE categorized_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Folder { .. } => "ri.lifecycle = 'active' AND EXISTS (
                 SELECT 1 FROM folder_item scoped_folder
                 WHERE scoped_folder.folder_id = ?1
                   AND scoped_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::SmartFolder { smart_folder_id } => {
            let (sql, values) = crate::smart_v2::compile_smart_folder_sql(
                connection,
                *smart_folder_id,
                arguments.len(),
            )?;
            arguments.extend(
                values
                    .into_iter()
                    .map(|value| Box::new(value) as Box<dyn ToSql>),
            );
            format!("ri.lifecycle = 'active' AND ri.item_id IN ({sql})")
        }
    };
    Ok(predicate)
}

fn apply_filters(
    filters: &ItemFilters,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(text) = filters.text.as_deref().filter(|text| !text.is_empty()) {
        let index = push_argument(arguments, format!("%{text}%"));
        predicates.push(format!(
            "(ri.label LIKE ?{index} OR EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id
                   AND (ma.name LIKE ?{index}
                        OR ma.notes LIKE ?{index}
                        OR ma.source_urls_json LIKE ?{index})
             ))"
        ));
    }

    if let Some(rating) = filters.minimum_rating {
        let index = push_argument(arguments, rating);
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id AND ma.rating >= ?{index}
             )"
        ));
    }

    if let Some(mime_prefix) = filters
        .mime_prefix
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let index = push_argument(arguments, format!("{mime_prefix}%"));
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE rm.root_item_id = ri.item_id AND mf.mime_type LIKE ?{index}
             )"
        ));
    }

    for tag in &filters.include_tags {
        let index = push_argument(arguments, tag.clone());
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 JOIN tag t ON t.tag_id = mt.tag_id
                 WHERE rm.root_item_id = ri.item_id
                   AND (t.namespace || ':' || t.subtag = ?{index}
                        OR t.subtag = ?{index})
             )"
        ));
    }

    for tag in &filters.exclude_tags {
        let index = push_argument(arguments, tag.clone());
        predicates.push(format!(
            "NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 JOIN tag t ON t.tag_id = mt.tag_id
                 WHERE rm.root_item_id = ri.item_id
                   AND (t.namespace || ':' || t.subtag = ?{index}
                        OR t.subtag = ?{index})
             )"
        ));
    }
}

fn push_argument<T: ToSql + 'static>(arguments: &mut Vec<Box<dyn ToSql>>, value: T) -> usize {
    arguments.push(Box::new(value));
    arguments.len()
}

fn order_expression(item_query: &ItemQuery, arguments: &mut Vec<Box<dyn ToSql>>) -> String {
    use crate::app::{ItemSortField, SortDirection};

    if matches!(item_query.scope, ItemScope::RecentlyViewed)
        && matches!(item_query.sort.field, ItemSortField::ImportedAt)
    {
        return "fr.viewed_at DESC".to_string();
    }

    let expression = match item_query.sort.field {
        ItemSortField::ImportedAt => "fr.imported_at",
        ItemSortField::CapturedAt => "fr.captured_at",
        ItemSortField::Name => "fr.sort_name",
        ItemSortField::Rating => "COALESCE(fr.sort_rating, -1)",
        ItemSortField::Size => "fr.total_size_bytes",
        ItemSortField::Random => {
            let seed = stable_seed(item_query.sort.random_seed.as_deref().unwrap_or_default());
            let index = push_argument(arguments, seed);
            return format!(
                "((fr.item_id * 1103515245 + ?{index}) & 2147483647) {}",
                match item_query.sort.direction {
                    SortDirection::Ascending => "ASC",
                    SortDirection::Descending => "DESC",
                }
            );
        }
        ItemSortField::FolderOrder => "COALESCE(fr.folder_position, 9223372036854775807)",
    };

    let direction = match item_query.sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    format!("{expression} {direction}")
}

fn stable_seed(value: &str) -> i64 {
    value.bytes().fold(2_166_136_261_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(16_777_619)
    }) as i64
}

#[cfg(test)]
mod tests {
    use super::{query, resolve_target_ids, ItemPageRequest};
    use crate::app::{ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemSort, ItemTarget};
    use crate::store::Store;

    fn query_for(scope: ItemScope) -> ItemQuery {
        ItemQuery {
            scope,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        }
    }

    fn seed_store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|tx| {
                insert_media(tx, 1, "standalone", "active", "one.jpg", "image/jpeg", 10, Some(2));
                insert_media(tx, 2, "inbox", "inbox", "inbox.jpg", "image/jpeg", 20, Some(1));
                insert_media(tx, 3, "trash", "trash", "trash.mp4", "video/mp4", 30, Some(4));
                insert_collection(tx, 10, "collection", "active", "Album");
                insert_media(tx, 11, "member-a", "active", "member-a.jpg", "image/jpeg", 40, Some(3));
                insert_media(tx, 12, "member-b", "active", "member-b.mp4", "video/mp4", 50, Some(5));
                tx.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 11, 0), (10, 12, 1)",
                    [],
                )?;
                tx.execute("INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at) VALUES (7, 'folder', 'Folder', 'now', 'now')", [])?;
                tx.execute("INSERT INTO folder_item (folder_id, item_id, position_rank) VALUES (7, 10, 0), (7, 1, 1)", [])?;
                tx.execute("INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'member-tag')", [])?;
                tx.execute("INSERT INTO media_tag (media_item_id, tag_id) VALUES (11, 1)", [])?;
                tx.execute("INSERT INTO media_view (item_id, viewed_at) VALUES (1, '2026-02-01')", [])?;
                Ok(())
            })
            .unwrap();
        (directory, store)
    }

    fn insert_collection(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        label: &str,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
             VALUES (?1, ?2, 'collection', ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key, label],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
    }

    fn insert_media(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        name: &str,
        mime_type: &str,
        size_bytes: i64,
        rating: Option<i64>,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
             VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01')",
            rusqlite::params![item_id, format!("hash-{item_id}"), mime_type, size_bytes],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_asset (item_id, file_id, name, rating, imported_at, updated_at)
             VALUES (?1, ?1, ?2, ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, name, rating],
        )
        .unwrap();
    }

    #[test]
    fn all_is_active_and_collapses_collection_members() {
        let (_directory, store) = seed_store();
        let page = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(page.items.len(), 2);
        let collection = page
            .items
            .iter()
            .find(|item| item.item_id == ItemId(10))
            .unwrap();
        assert_eq!(collection.kind, ItemKind::Collection);
        assert_eq!(collection.collection_member_count, 2);
        assert_eq!(collection.total_size_bytes, 90);
        assert_eq!(page.visible_item_count, 2);
        assert_eq!(page.visible_media_count, 3);
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(11)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(12)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(2)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(3)));
    }

    #[test]
    fn inbox_and_trash_are_separate_root_scopes() {
        let (_directory, store) = seed_store();
        let inbox = query(
            &store,
            &query_for(ItemScope::Inbox),
            ItemPageRequest::default(),
        )
        .unwrap();
        let trash = query(
            &store,
            &query_for(ItemScope::Trash),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(
            inbox
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(2)]
        );
        assert_eq!(
            trash
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(3)]
        );
        assert_eq!(inbox.visible_item_count, 1);
        assert_eq!(trash.visible_media_count, 1);
    }

    #[test]
    fn folder_scope_is_active_and_root_only() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::Folder { folder_id: 7 });
        item_query.sort.field = crate::app::ItemSortField::FolderOrder;
        item_query.sort.direction = crate::app::SortDirection::Ascending;
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10), ItemId(1)]
        );
        assert_eq!(page.visible_item_count, 2);
        assert_eq!(page.visible_media_count, 3);
    }

    #[test]
    fn media_filters_project_member_matches_to_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.include_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
        assert_eq!(page.visible_item_count, 1);
        assert_eq!(page.visible_media_count, 2);
    }

    #[test]
    fn revision_and_counts_survive_empty_pages() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.text = Some("does-not-exist".to_string());
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert!(page.items.is_empty());
        assert_eq!(page.visible_item_count, 0);
        assert_eq!(page.visible_media_count, 0);
        assert_eq!(page.revision, 1);
    }

    #[test]
    fn active_only_derived_scopes_use_the_same_root_set() {
        let (_directory, store) = seed_store();

        let recent = query(
            &store,
            &query_for(ItemScope::RecentlyViewed),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(recent.items[0].item_id, ItemId(1));

        let untagged = query(
            &store,
            &query_for(ItemScope::Untagged),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(
            untagged
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );

        let uncategorized = query(
            &store,
            &query_for(ItemScope::Uncategorized),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert!(uncategorized.items.is_empty());
    }

    #[test]
    fn excluded_member_tag_excludes_the_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.exclude_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );
    }

    #[test]
    fn seeded_random_order_is_stable() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.sort.field = crate::app::ItemSortField::Random;
        item_query.sort.random_seed = Some("stable".to_string());

        let first = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        let second = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(first.items, second.items);
    }

    #[test]
    fn smart_folder_scope_uses_the_same_page_and_count_query() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Tagged', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:member-tag"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                Ok(())
            })
            .unwrap();

        let page = query(
            &store,
            &query_for(ItemScope::SmartFolder { smart_folder_id: 9 }),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(page.visible_item_count, 1);
        assert_eq!(page.visible_media_count, 2);
        assert_eq!(page.items[0].item_id, ItemId(10));
    }

    #[test]
    fn query_mutation_target_matches_grid_and_applies_exclusions() {
        let (_directory, store) = seed_store();
        let target = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: vec![ItemId(1)],
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![10]);
    }
}
