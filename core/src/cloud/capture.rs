use std::collections::{BTreeMap, BTreeSet};

use fallible_streaming_iterator::FallibleStreamingIterator;
use rusqlite::hooks::Action;
use rusqlite::session::{ChangesetIter, Session};
use rusqlite::types::ValueRef;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use super::{
    json_sql_error, record_local, source_item_sync_key, source_post_sync_key, CloudFolder,
    CloudOperation, CloudSmartFolder, CloudSourceItem, CloudSourcePost, CloudSubscription,
    CloudSubscriptionQuery, RestoredItem, RestoredMedia,
};

const CAPTURED_TABLES: &[&str] = &[
    "library_item",
    "library_root",
    "media_asset",
    "media_tag",
    "folder",
    "folder_item",
    "smart_folder",
    "collection_member",
];

/// Converts locally observed row changes into one typed cloud envelope. The
/// SQLite changeset is discarded before commit and is never replicated.
pub struct SemanticCapture<'connection> {
    session: Option<Session<'connection>>,
}

impl<'connection> SemanticCapture<'connection> {
    pub fn start(transaction: &'connection Transaction<'_>) -> rusqlite::Result<Self> {
        let configured: bool = transaction.query_row(
            "SELECT provider IS NOT NULL FROM cloud_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if !configured {
            return Ok(Self { session: None });
        }
        begin_explicit_capture(transaction)?;
        let mut session = Session::new(transaction)?;
        for table in CAPTURED_TABLES {
            session.attach(Some(table))?;
        }
        Ok(Self {
            session: Some(session),
        })
    }

    pub fn finish(mut self, transaction: &Transaction<'_>) -> rusqlite::Result<usize> {
        let Some(mut session) = self.session.take() else {
            return Ok(0);
        };
        let mut changeset = Vec::new();
        if !session.is_empty() {
            session.changeset_strm(&mut changeset)?;
        }
        drop(session);
        finish_captured(
            transaction,
            (!changeset.is_empty()).then_some(changeset.as_slice()),
        )
    }
}

pub(crate) fn begin_explicit_capture(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS cloud_capture_operation (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             payload_json TEXT NOT NULL
         );
         DELETE FROM cloud_capture_operation;",
    )
}

pub(crate) fn finish_explicit_capture(
    transaction: &Transaction<'_>,
    changeset: Option<&[u8]>,
) -> rusqlite::Result<usize> {
    finish_captured(transaction, changeset)
}

fn finish_captured(
    transaction: &Transaction<'_>,
    changeset: Option<&[u8]>,
) -> rusqlite::Result<usize> {
    let staged = staged_operations(transaction)?;
    if changeset.is_none() && staged.is_empty() {
        return Ok(0);
    }
    let configured: bool = transaction.query_row(
        "SELECT provider IS NOT NULL FROM cloud_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if !configured {
        return Ok(0);
    }
    let mut item_ids = BTreeSet::new();
    let mut inserted_item_ids = BTreeSet::new();
    let mut deleted_item_keys = BTreeSet::new();
    let mut tag_changes = BTreeMap::new();
    let mut folder_changes = BTreeMap::new();
    let mut changed_folders = BTreeSet::new();
    let mut deleted_folder_keys = BTreeSet::new();
    let mut changed_smart_folders = BTreeSet::new();
    let mut deleted_smart_folder_keys = BTreeSet::new();
    let mut group_changes = BTreeSet::new();
    let explicit_folder_keys = staged
        .iter()
        .flat_map(flatten_operation)
        .filter_map(|operation| match operation {
            CloudOperation::UpsertFolder { folder, .. } => {
                Some(("folder", folder.folder_key.as_str()))
            }
            CloudOperation::DeleteFolder { folder_key } => Some(("folder", folder_key.as_str())),
            CloudOperation::UpsertSmartFolder { smart_folder, .. } => {
                Some(("smart_folder", smart_folder.smart_folder_key.as_str()))
            }
            CloudOperation::DeleteSmartFolder { smart_folder_key } => {
                Some(("smart_folder", smart_folder_key.as_str()))
            }
            _ => None,
        })
        .map(|(kind, key)| (kind.to_string(), key.to_string()))
        .collect::<BTreeSet<_>>();
    let has_explicit_folders = explicit_folder_keys
        .iter()
        .any(|(kind, _)| kind == "folder");
    let has_explicit_smart_folders = explicit_folder_keys
        .iter()
        .any(|(kind, _)| kind == "smart_folder");

    if let Some(mut changeset) = changeset {
        let input: &mut dyn std::io::Read = &mut changeset;
        let mut iterator = ChangesetIter::start_strm(&input)?;
        while let Some(change) = iterator.next()? {
            let operation = change.op()?;
            match operation.table_name() {
                "library_item" => {
                    if let Some(item_id) = row_integer(change, 0, operation.code())? {
                        item_ids.insert(item_id);
                        if operation.code() == Action::SQLITE_INSERT {
                            inserted_item_ids.insert(item_id);
                        }
                    }
                    if operation.code() == Action::SQLITE_DELETE {
                        if let ValueRef::Text(value) = change.old_value(1)? {
                            deleted_item_keys.insert(String::from_utf8_lossy(value).into_owned());
                        }
                    }
                }
                "library_root" | "media_asset" => {
                    if let Some(item_id) = row_integer(change, 0, operation.code())? {
                        item_ids.insert(item_id);
                    }
                }
                "media_tag" => {
                    let media_id = row_integer(change, 0, operation.code())?;
                    let tag_id = row_integer(change, 1, operation.code())?;
                    if let (Some(media_id), Some(tag_id)) = (media_id, tag_id) {
                        tag_changes.insert(
                            (media_id, tag_id),
                            operation.code() != Action::SQLITE_DELETE,
                        );
                    }
                }
                "folder_item" => {
                    let folder_id = row_integer(change, 0, operation.code())?;
                    let item_id = row_integer(change, 1, operation.code())?;
                    if let (Some(folder_id), Some(item_id)) = (folder_id, item_id) {
                        folder_changes.insert(
                            (folder_id, item_id),
                            operation.code() != Action::SQLITE_DELETE,
                        );
                    }
                }
                "folder" => {
                    if has_explicit_folders {
                        continue;
                    }
                    if operation.code() == Action::SQLITE_DELETE {
                        if let Some(folder_key) = row_text(change, 1, operation.code())? {
                            deleted_folder_keys.insert(folder_key);
                        }
                    } else if let Some(folder_id) = row_integer(change, 0, operation.code())? {
                        changed_folders.insert(folder_id);
                    }
                }
                "smart_folder" => {
                    if has_explicit_smart_folders {
                        continue;
                    }
                    if operation.code() == Action::SQLITE_DELETE {
                        if let Some(smart_folder_key) = row_text(change, 1, operation.code())? {
                            deleted_smart_folder_keys.insert(smart_folder_key);
                        }
                    } else if let Some(smart_folder_id) = row_integer(change, 0, operation.code())?
                    {
                        changed_smart_folders.insert(smart_folder_id);
                    }
                }
                "collection_member" => {
                    if let Some(media_id) = row_integer(change, 1, operation.code())? {
                        group_changes.insert(media_id);
                    }
                }
                _ => {}
            }
        }
    }

    let mut source_operations = staged
        .iter()
        .flat_map(flatten_operation)
        .filter(|operation| is_source_operation(operation))
        .cloned()
        .collect::<Vec<_>>();
    let mut operations = staged
        .iter()
        .flat_map(flatten_operation)
        .filter(|operation| !is_source_operation(operation))
        .cloned()
        .collect::<Vec<_>>();
    let mut folders = changed_folders
        .into_iter()
        .filter_map(|folder_id| folder_state(transaction, folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?;
    folders.retain(|folder| {
        !explicit_folder_keys.contains(&("folder".into(), folder.folder_key.clone()))
    });
    folders.sort_by_key(|folder| folder_depth(transaction, &folder.folder_key).unwrap_or(0));
    operations.extend(
        folders
            .into_iter()
            .map(|folder| CloudOperation::UpsertFolder {
                folder,
                changed_fields: folder_fields(),
            }),
    );
    operations.extend(
        deleted_folder_keys
            .into_iter()
            .filter(|key| !explicit_folder_keys.contains(&("folder".into(), key.clone())))
            .map(|folder_key| CloudOperation::DeleteFolder { folder_key }),
    );
    let mut smart_folders = changed_smart_folders
        .into_iter()
        .filter_map(|folder_id| smart_folder_state(transaction, folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?;
    smart_folders.retain(|folder| {
        !explicit_folder_keys.contains(&("smart_folder".into(), folder.smart_folder_key.clone()))
    });
    smart_folders.sort_by_key(|folder| {
        smart_folder_depth(transaction, &folder.smart_folder_key).unwrap_or(0)
    });
    operations.extend(smart_folders.into_iter().map(|smart_folder| {
        CloudOperation::UpsertSmartFolder {
            smart_folder,
            changed_fields: smart_folder_fields(),
        }
    }));
    operations.extend(
        deleted_smart_folder_keys
            .into_iter()
            .filter(|key| !explicit_folder_keys.contains(&("smart_folder".into(), key.clone())))
            .map(|smart_folder_key| CloudOperation::DeleteSmartFolder { smart_folder_key }),
    );
    for item_key in deleted_item_keys {
        operations.push(CloudOperation::DeleteItem { item_key });
    }
    for item_id in &inserted_item_ids {
        if let Some(item) = restored_item_state(transaction, *item_id)? {
            operations.push(CloudOperation::UpsertItem { item });
        }
    }
    for item_id in item_ids.difference(&inserted_item_ids) {
        if let Some(operation) = item_state(transaction, *item_id)? {
            operations.extend(operation);
        }
    }
    if !tag_changes.is_empty() {
        let changes = serde_json::to_string(
            &tag_changes
                .into_iter()
                .map(|((media_id, tag_id), present)| (media_id, tag_id, present))
                .collect::<Vec<_>>(),
        )
        .map_err(json_sql_error)?;
        let memberships = transaction
            .prepare(
                "SELECT li.item_key, t.namespace, t.subtag,
                        CAST(json_extract(change.value, '$[2]') AS INTEGER)
                 FROM json_each(?1) change
                 JOIN library_item li
                   ON li.item_id = CAST(json_extract(change.value, '$[0]') AS INTEGER)
                 JOIN tag t
                   ON t.tag_id = CAST(json_extract(change.value, '$[1]') AS INTEGER)",
            )?
            .query_map([changes], |row| {
                Ok(CloudOperation::TagMembership {
                    item_key: row.get(0)?,
                    namespace: row.get(1)?,
                    subtag: row.get(2)?,
                    present: row.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        operations.extend(memberships);
    }
    if !folder_changes.is_empty() {
        let changes = serde_json::to_string(
            &folder_changes
                .into_iter()
                .map(|((folder_id, item_id), present)| (folder_id, item_id, present))
                .collect::<Vec<_>>(),
        )
        .map_err(json_sql_error)?;
        let memberships = transaction
            .prepare(
                "SELECT li.item_key, f.folder_key,
                        CAST(json_extract(change.value, '$[2]') AS INTEGER),
                        fi.position_rank
                 FROM json_each(?1) change
                 JOIN folder f
                   ON f.folder_id = CAST(json_extract(change.value, '$[0]') AS INTEGER)
                 JOIN library_item li
                   ON li.item_id = CAST(json_extract(change.value, '$[1]') AS INTEGER)
                 LEFT JOIN folder_item fi
                   ON fi.folder_id = f.folder_id AND fi.item_id = li.item_id",
            )?
            .query_map([changes], |row| {
                Ok(CloudOperation::FolderMembership {
                    item_key: row.get(0)?,
                    folder_key: row.get(1)?,
                    present: row.get::<_, i64>(2)? != 0,
                    position_rank: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        operations.extend(memberships);
    }
    for media_id in group_changes {
        if let Some(operation) = group_state(transaction, media_id)? {
            operations.push(operation);
        }
    }
    // Source links may reference items created in this transaction, so they
    // follow item creation and group assignment in the same envelope.
    operations.append(&mut source_operations);
    if operations.is_empty() {
        return Ok(0);
    }
    let count = operations.len();
    record_local(transaction, CloudOperation::Batch { operations })?;
    Ok(count)
}

pub(crate) fn subscription_state(
    transaction: &Transaction<'_>,
    subscription_id: i64,
) -> rusqlite::Result<Option<CloudSubscription>> {
    transaction
        .query_row(
            "SELECT subscription_key, name, schedule, paused, initial_post_limit,
                    periodic_post_limit, created_at
             FROM subscription WHERE subscription_id = ?1",
            [subscription_id],
            |row| {
                Ok(CloudSubscription {
                    subscription_key: row.get(0)?,
                    name: row.get(1)?,
                    schedule: row.get(2)?,
                    paused: row.get::<_, i64>(3)? != 0,
                    initial_post_limit: row.get(4)?,
                    periodic_post_limit: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
        .optional()
}

pub(crate) fn subscription_query_state(
    transaction: &Transaction<'_>,
    query_id: i64,
) -> rusqlite::Result<Option<CloudSubscriptionQuery>> {
    transaction
        .query_row(
            "SELECT q.query_key, s.subscription_key, q.site_id, q.domain_key, q.query_kind,
                    q.query_text, q.display_name, q.notes, q.group_posts, q.paused
             FROM subscription_query q
             JOIN subscription s ON s.subscription_id = q.subscription_id
             WHERE q.query_id = ?1",
            [query_id],
            |row| {
                Ok(CloudSubscriptionQuery {
                    query_key: row.get(0)?,
                    subscription_key: row.get(1)?,
                    site_id: row.get(2)?,
                    domain_key: row.get(3)?,
                    query_kind: row.get(4)?,
                    query_text: row.get(5)?,
                    display_name: row.get(6)?,
                    notes: row.get(7)?,
                    group_posts: row.get::<_, i64>(8)? != 0,
                    paused: row.get::<_, i64>(9)? != 0,
                })
            },
        )
        .optional()
}

pub(crate) fn record_subscription_created(
    transaction: &Transaction<'_>,
    subscription_id: i64,
    query_ids: &[i64],
) -> rusqlite::Result<()> {
    let mut operations = Vec::with_capacity(query_ids.len() + 1);
    if let Some(subscription) = subscription_state(transaction, subscription_id)? {
        operations.push(CloudOperation::UpsertSubscription {
            subscription,
            changed_fields: subscription_fields(),
        });
    }
    for query_id in query_ids {
        if let Some(query) = subscription_query_state(transaction, *query_id)? {
            operations.push(CloudOperation::UpsertSubscriptionQuery {
                query,
                changed_fields: subscription_query_fields(),
            });
        }
    }
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_subscription_upsert(
    transaction: &Transaction<'_>,
    subscription_id: i64,
    fields: &[&str],
) -> rusqlite::Result<()> {
    let Some(subscription) = subscription_state(transaction, subscription_id)? else {
        return Ok(());
    };
    record_if_configured(
        transaction,
        CloudOperation::UpsertSubscription {
            subscription,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        },
    )
}

pub(crate) fn record_subscription_delete(
    transaction: &Transaction<'_>,
    subscription_key: String,
) -> rusqlite::Result<()> {
    record_if_configured(
        transaction,
        CloudOperation::DeleteSubscription { subscription_key },
    )
}

pub(crate) fn record_subscription_query_upsert(
    transaction: &Transaction<'_>,
    query_id: i64,
    fields: &[&str],
) -> rusqlite::Result<()> {
    let Some(query) = subscription_query_state(transaction, query_id)? else {
        return Ok(());
    };
    record_if_configured(
        transaction,
        CloudOperation::UpsertSubscriptionQuery {
            query,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        },
    )
}

pub(crate) fn record_subscription_query_delete(
    transaction: &Transaction<'_>,
    query_key: String,
) -> rusqlite::Result<()> {
    record_if_configured(
        transaction,
        CloudOperation::DeleteSubscriptionQuery { query_key },
    )
}

fn source_post_state(
    transaction: &Transaction<'_>,
    source_post_id: i64,
) -> rusqlite::Result<Option<CloudSourcePost>> {
    transaction
        .query_row(
            "SELECT sp.site_id, sp.post_key, sp.canonical_url, sp.creator_name, sp.title,
                    sp.description, sp.captured_at, sp.metadata_json, root.item_key,
                    sp.created_at, sp.updated_at
             FROM source_post sp
             LEFT JOIN library_item root ON root.item_id = sp.root_item_id
             WHERE sp.source_post_id = ?1",
            [source_post_id],
            |row| {
                let site_id: String = row.get(0)?;
                let post_key: String = row.get(1)?;
                Ok(CloudSourcePost {
                    source_post_key: source_post_sync_key(&site_id, &post_key),
                    site_id,
                    post_key,
                    canonical_url: row.get(2)?,
                    creator_name: row.get(3)?,
                    title: row.get(4)?,
                    description: row.get(5)?,
                    captured_at: row.get(6)?,
                    metadata_json: row.get(7)?,
                    root_item_key: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            },
        )
        .optional()
}

fn source_item_state(
    transaction: &Transaction<'_>,
    source_item_id: i64,
) -> rusqlite::Result<Option<CloudSourceItem>> {
    transaction
        .query_row(
            "SELECT sp.site_id, sp.post_key, si.item_key, si.position, si.media_url,
                    si.canonical_url, media.item_key, si.created_at, si.updated_at
             FROM source_item si
             JOIN source_post sp ON sp.source_post_id = si.source_post_id
             LEFT JOIN library_item media ON media.item_id = si.media_item_id
             WHERE si.source_item_id = ?1",
            [source_item_id],
            |row| {
                let site_id: String = row.get(0)?;
                let post_key: String = row.get(1)?;
                let item_key: String = row.get(2)?;
                Ok(CloudSourceItem {
                    source_item_key: source_item_sync_key(&site_id, &post_key, &item_key),
                    source_post_key: source_post_sync_key(&site_id, &post_key),
                    item_key,
                    position: row.get(3)?,
                    media_url: row.get(4)?,
                    canonical_url: row.get(5)?,
                    media_item_key: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .optional()
}

pub(crate) fn record_source_post_upsert(
    transaction: &Transaction<'_>,
    source_post_id: i64,
    fields: &[&str],
) -> rusqlite::Result<()> {
    let Some(source_post) = source_post_state(transaction, source_post_id)? else {
        return Ok(());
    };
    record_if_configured(
        transaction,
        CloudOperation::UpsertSourcePost {
            source_post,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        },
    )
}

pub(crate) fn record_source_item_upsert(
    transaction: &Transaction<'_>,
    source_item_id: i64,
    fields: &[&str],
) -> rusqlite::Result<()> {
    let Some(source_item) = source_item_state(transaction, source_item_id)? else {
        return Ok(());
    };
    record_if_configured(
        transaction,
        CloudOperation::UpsertSourceItem {
            source_item,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        },
    )
}

pub(crate) fn record_source_item_deletes(
    transaction: &Transaction<'_>,
    source_item_ids: &[i64],
) -> rusqlite::Result<()> {
    let operations = source_item_ids
        .iter()
        .filter_map(|source_item_id| source_item_state(transaction, *source_item_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|source_item| CloudOperation::DeleteSourceItem {
            source_item_key: source_item.source_item_key,
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_subscription_source_item_restores(
    transaction: &Transaction<'_>,
    subscription_id: i64,
) -> rusqlite::Result<()> {
    let source_item_ids = transaction
        .prepare(
            "SELECT DISTINCT si.source_item_id
             FROM subscription_source_post ssp
             JOIN source_item si ON si.source_post_id = ssp.source_post_id
             WHERE ssp.subscription_id = ?1 AND si.media_item_id IS NULL",
        )?
        .query_map([subscription_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut operations = Vec::new();
    for source_item_id in source_item_ids {
        let Some(source_item) = source_item_state(transaction, source_item_id)? else {
            continue;
        };
        let tombstone_mutation_id: Option<String> = transaction
            .query_row(
                "SELECT mutation_id FROM cloud_tombstone
                 WHERE object_kind = 'source_item' AND object_key = ?1",
                [&source_item.source_item_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(tombstone_mutation_id) = tombstone_mutation_id {
            operations.push(CloudOperation::RestoreSourceItem {
                tombstone_mutation_id,
                source_item,
            });
        }
    }
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_subscription_source_post(
    transaction: &Transaction<'_>,
    subscription_id: i64,
    query_id: i64,
    source_post_id: i64,
    present: bool,
) -> rusqlite::Result<()> {
    let identity: Option<(String, String, String, String)> = transaction
        .query_row(
            "SELECT s.subscription_key, q.query_key, sp.site_id, sp.post_key
             FROM subscription s
             JOIN subscription_query q ON q.subscription_id = s.subscription_id
             CROSS JOIN source_post sp
             WHERE s.subscription_id = ?1 AND q.query_id = ?2 AND sp.source_post_id = ?3",
            params![subscription_id, query_id, source_post_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((subscription_key, query_key, site_id, post_key)) = identity else {
        return Ok(());
    };
    record_if_configured(
        transaction,
        CloudOperation::SubscriptionSourcePost {
            subscription_key,
            query_key,
            source_post_key: source_post_sync_key(&site_id, &post_key),
            present,
        },
    )
}

pub(crate) fn record_subscription_source_posts_removed(
    transaction: &Transaction<'_>,
    subscription_id: i64,
) -> rusqlite::Result<()> {
    let rows = transaction
        .prepare(
            "SELECT s.subscription_key, q.query_key, sp.site_id, sp.post_key
             FROM subscription_source_post ssp
             JOIN subscription s ON s.subscription_id = ssp.subscription_id
             JOIN subscription_query q ON q.query_id = ssp.query_id
             JOIN source_post sp ON sp.source_post_id = ssp.source_post_id
             WHERE ssp.subscription_id = ?1",
        )?
        .query_map([subscription_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let operations = rows
        .into_iter()
        .map(|(subscription_key, query_key, site_id, post_key)| {
            CloudOperation::SubscriptionSourcePost {
                subscription_key,
                query_key,
                source_post_key: source_post_sync_key(&site_id, &post_key),
                present: false,
            }
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

fn record_if_configured(
    transaction: &Transaction<'_>,
    operation: CloudOperation,
) -> rusqlite::Result<()> {
    let configured: bool = transaction.query_row(
        "SELECT provider IS NOT NULL FROM cloud_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    if configured {
        let payload_json = serde_json::to_string(&operation).map_err(|error| {
            rusqlite::Error::InvalidParameterName(format!(
                "could not encode staged cloud operation: {error}"
            ))
        })?;
        transaction.execute(
            "INSERT INTO cloud_capture_operation (payload_json) VALUES (?1)",
            [payload_json],
        )?;
    }
    Ok(())
}

fn staged_operations(transaction: &Transaction<'_>) -> rusqlite::Result<Vec<CloudOperation>> {
    transaction
        .prepare("SELECT payload_json FROM cloud_capture_operation ORDER BY sequence")?
        .query_map([], |row| {
            let payload: String = row.get(0)?;
            serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })?
        .collect()
}

fn flatten_operation(operation: &CloudOperation) -> Box<dyn Iterator<Item = &CloudOperation> + '_> {
    match operation {
        CloudOperation::Batch { operations } => {
            Box::new(operations.iter().flat_map(flatten_operation))
        }
        operation => Box::new(std::iter::once(operation)),
    }
}

fn is_source_operation(operation: &CloudOperation) -> bool {
    matches!(
        operation,
        CloudOperation::UpsertSourcePost { .. }
            | CloudOperation::UpsertSourceItem { .. }
            | CloudOperation::DeleteSourceItem { .. }
            | CloudOperation::RestoreSourceItem { .. }
            | CloudOperation::SubscriptionSourcePost { .. }
    )
}

fn subscription_fields() -> Vec<String> {
    [
        "name",
        "schedule",
        "paused",
        "initial_post_limit",
        "periodic_post_limit",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn subscription_query_fields() -> Vec<String> {
    [
        "subscription",
        "site_id",
        "domain_key",
        "query_kind",
        "query_text",
        "display_name",
        "notes",
        "group_posts",
        "paused",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn folder_fields() -> Vec<String> {
    [
        "exists",
        "name",
        "parent",
        "icon",
        "color",
        "notes",
        "sort_rank",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn smart_folder_fields() -> Vec<String> {
    [
        "exists",
        "name",
        "parent",
        "icon",
        "color",
        "notes",
        "predicate_json",
        "sort_field",
        "sort_order",
        "display_order",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(crate) fn record_folder_created(
    transaction: &Transaction<'_>,
    folder_ids: &[i64],
) -> rusqlite::Result<()> {
    let mut folders = folder_ids
        .iter()
        .filter_map(|folder_id| folder_state(transaction, *folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?;
    folders.sort_by_key(|folder| folder_depth(transaction, &folder.folder_key).unwrap_or(0));
    let operations = folders
        .into_iter()
        .map(|folder| CloudOperation::UpsertFolder {
            folder,
            changed_fields: folder_fields(),
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_folder_upsert(
    transaction: &Transaction<'_>,
    folder_ids: &[i64],
    fields: &[&str],
) -> rusqlite::Result<()> {
    let operations = folder_ids
        .iter()
        .filter_map(|folder_id| folder_state(transaction, *folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|folder| CloudOperation::UpsertFolder {
            folder,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_folder_delete(
    transaction: &Transaction<'_>,
    folder_keys: Vec<String>,
) -> rusqlite::Result<()> {
    let operations = folder_keys
        .into_iter()
        .map(|folder_key| CloudOperation::DeleteFolder { folder_key })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_smart_folder_created(
    transaction: &Transaction<'_>,
    smart_folder_ids: &[i64],
) -> rusqlite::Result<()> {
    let mut folders = smart_folder_ids
        .iter()
        .filter_map(|folder_id| smart_folder_state(transaction, *folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?;
    folders.sort_by_key(|folder| {
        smart_folder_depth(transaction, &folder.smart_folder_key).unwrap_or(0)
    });
    let operations = folders
        .into_iter()
        .map(|smart_folder| CloudOperation::UpsertSmartFolder {
            smart_folder,
            changed_fields: smart_folder_fields(),
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_smart_folder_upsert(
    transaction: &Transaction<'_>,
    smart_folder_ids: &[i64],
    fields: &[&str],
) -> rusqlite::Result<()> {
    let operations = smart_folder_ids
        .iter()
        .filter_map(|folder_id| smart_folder_state(transaction, *folder_id).transpose())
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|smart_folder| CloudOperation::UpsertSmartFolder {
            smart_folder,
            changed_fields: fields.iter().map(|field| (*field).to_string()).collect(),
        })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

pub(crate) fn record_smart_folder_delete(
    transaction: &Transaction<'_>,
    smart_folder_keys: Vec<String>,
) -> rusqlite::Result<()> {
    let operations = smart_folder_keys
        .into_iter()
        .map(|smart_folder_key| CloudOperation::DeleteSmartFolder { smart_folder_key })
        .collect();
    record_if_configured(transaction, CloudOperation::Batch { operations })
}

fn folder_state(
    transaction: &Transaction<'_>,
    folder_id: i64,
) -> rusqlite::Result<Option<CloudFolder>> {
    transaction
        .query_row(
            "SELECT f.folder_key, f.name, parent.folder_key, f.icon, f.color, f.notes,
                    f.sort_rank, f.created_at, f.updated_at
             FROM folder f
             LEFT JOIN folder parent ON parent.folder_id = f.parent_id
             WHERE f.folder_id = ?1",
            [folder_id],
            |row| {
                Ok(CloudFolder {
                    folder_key: row.get(0)?,
                    name: row.get(1)?,
                    parent_key: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    sort_rank: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .optional()
}

fn smart_folder_state(
    transaction: &Transaction<'_>,
    smart_folder_id: i64,
) -> rusqlite::Result<Option<CloudSmartFolder>> {
    transaction
        .query_row(
            "SELECT sf.smart_folder_key, sf.name, parent.smart_folder_key, sf.icon, sf.color,
                    sf.notes, sf.predicate_json, sf.sort_field, sf.sort_order, sf.display_order,
                    sf.created_at, sf.updated_at
             FROM smart_folder sf
             LEFT JOIN smart_folder parent ON parent.smart_folder_id = sf.parent_id
             WHERE sf.smart_folder_id = ?1",
            [smart_folder_id],
            |row| {
                Ok(CloudSmartFolder {
                    smart_folder_key: row.get(0)?,
                    name: row.get(1)?,
                    parent_key: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    predicate_json: row.get(6)?,
                    sort_field: row.get(7)?,
                    sort_order: row.get(8)?,
                    display_order: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()
}

fn folder_depth(transaction: &Transaction<'_>, folder_key: &str) -> rusqlite::Result<i64> {
    transaction.query_row(
        "WITH RECURSIVE ancestors(folder_id, depth) AS (
             SELECT folder_id, 0 FROM folder WHERE folder_key = ?1
             UNION ALL
             SELECT parent.folder_id, ancestors.depth + 1
             FROM ancestors
             JOIN folder child ON child.folder_id = ancestors.folder_id
             JOIN folder parent ON parent.folder_id = child.parent_id
         ) SELECT COALESCE(MAX(depth), 0) FROM ancestors",
        [folder_key],
        |row| row.get(0),
    )
}

fn smart_folder_depth(
    transaction: &Transaction<'_>,
    smart_folder_key: &str,
) -> rusqlite::Result<i64> {
    transaction.query_row(
        "WITH RECURSIVE ancestors(smart_folder_id, depth) AS (
             SELECT smart_folder_id, 0 FROM smart_folder WHERE smart_folder_key = ?1
             UNION ALL
             SELECT parent.smart_folder_id, ancestors.depth + 1
             FROM ancestors
             JOIN smart_folder child ON child.smart_folder_id = ancestors.smart_folder_id
             JOIN smart_folder parent ON parent.smart_folder_id = child.parent_id
         ) SELECT COALESCE(MAX(depth), 0) FROM ancestors",
        [smart_folder_key],
        |row| row.get(0),
    )
}

fn restored_item_state(
    transaction: &Transaction<'_>,
    item_id: i64,
) -> rusqlite::Result<Option<RestoredItem>> {
    let item: Option<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = transaction
        .query_row(
            "SELECT li.item_key, li.kind, li.label,
                    (SELECT cover.item_key FROM library_item cover
                     WHERE cover.item_id = li.cover_media_item_id),
                    (SELECT lifecycle FROM library_root WHERE item_id = li.item_id)
             FROM library_item li WHERE li.item_id = ?1",
            [item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((item_key, kind, label, cover_media_item_key, lifecycle)) = item else {
        return Ok(None);
    };
    let media = if kind == "media" {
        transaction
            .query_row(
                "SELECT mf.file_hash, mf.mime_type, mf.size_bytes, mf.pixel_width,
                        mf.pixel_height, mf.duration_ms, mf.frame_count, mf.has_audio,
                        ma.name, ma.notes, ma.rating, ma.source_urls_json, ma.captured_at,
                        ma.imported_at
                 FROM media_asset ma
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE ma.item_id = ?1",
                [item_id],
                |row| {
                    Ok(RestoredMedia {
                        file_hash: row.get(0)?,
                        mime_type: row.get(1)?,
                        size_bytes: row.get(2)?,
                        pixel_width: row.get(3)?,
                        pixel_height: row.get(4)?,
                        duration_ms: row.get(5)?,
                        frame_count: row.get(6)?,
                        has_audio: row.get::<_, i64>(7)? != 0,
                        name: row.get(8)?,
                        notes: row.get(9)?,
                        rating: row.get(10)?,
                        source_urls_json: row.get(11)?,
                        captured_at: row.get(12)?,
                        imported_at: row.get(13)?,
                    })
                },
            )
            .optional()?
    } else {
        None
    };
    if kind == "media" && media.is_none() {
        return Ok(None);
    }
    Ok(Some(RestoredItem {
        item_key,
        kind,
        label,
        cover_media_item_key,
        lifecycle: lifecycle.unwrap_or_else(|| "active".to_string()),
        media,
    }))
}

fn item_state(
    transaction: &Transaction<'_>,
    item_id: i64,
) -> rusqlite::Result<Option<Vec<CloudOperation>>> {
    let item: Option<(String, Option<String>, Option<String>)> = transaction
        .query_row(
            "SELECT item_key, label,
                    (SELECT lifecycle FROM library_root WHERE item_id = library_item.item_id)
             FROM library_item WHERE item_id = ?1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((item_key, label, lifecycle)) = item else {
        return Ok(None);
    };
    let mut fields = BTreeMap::from([("label".to_string(), optional_string(label))]);
    let media: Option<(
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
    )> = transaction
        .query_row(
            "SELECT name, notes, rating, source_urls_json, captured_at
             FROM media_asset WHERE item_id = ?1",
            [item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    if let Some((name, notes, rating, source_urls, captured_at)) = media {
        fields.insert("name".into(), optional_string(name));
        fields.insert("notes".into(), optional_string(notes));
        fields.insert(
            "rating".into(),
            rating.map(Value::from).unwrap_or(Value::Null),
        );
        fields.insert("source_urls_json".into(), optional_string(source_urls));
        fields.insert("captured_at".into(), optional_string(captured_at));
    }
    let mut operations = vec![CloudOperation::ItemFields {
        item_key: item_key.clone(),
        fields,
    }];
    if let Some(lifecycle) = lifecycle {
        operations.push(CloudOperation::Lifecycle {
            item_key,
            lifecycle,
        });
    }
    Ok(Some(operations))
}

fn group_state(
    transaction: &Transaction<'_>,
    media_id: i64,
) -> rusqlite::Result<Option<CloudOperation>> {
    let media_key: Option<String> = transaction
        .query_row(
            "SELECT item_key FROM library_item WHERE item_id = ?1",
            [media_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(media_item_key) = media_key else {
        return Ok(None);
    };
    let collection: Option<(String, i64)> = transaction
        .query_row(
            "SELECT li.item_key, cm.position_rank
             FROM collection_member cm
             JOIN library_item li ON li.item_id = cm.collection_id
             WHERE cm.media_item_id = ?1",
            [media_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(Some(CloudOperation::GroupAssignment {
        media_item_key,
        collection_item_key: collection.as_ref().map(|value| value.0.clone()),
        position_rank: collection.map(|value| value.1),
    }))
}

fn row_integer(
    change: &rusqlite::session::ChangesetItem,
    column: usize,
    action: Action,
) -> rusqlite::Result<Option<i64>> {
    let value = match action {
        Action::SQLITE_DELETE => change.old_value(column)?,
        Action::SQLITE_INSERT => change.new_value(column)?,
        // Session changesets omit new values for unchanged UPDATE columns. All
        // integer columns read here are stable row identities, so old.* is the
        // authoritative value and avoids handing rusqlite a null SQLite value.
        Action::SQLITE_UPDATE => change.old_value(column)?,
        _ => return Ok(None),
    };
    Ok(match value {
        ValueRef::Integer(value) => Some(value),
        _ => None,
    })
}

fn row_text(
    change: &rusqlite::session::ChangesetItem,
    column: usize,
    action: Action,
) -> rusqlite::Result<Option<String>> {
    let value = match action {
        Action::SQLITE_DELETE => change.old_value(column)?,
        Action::SQLITE_INSERT => change.new_value(column)?,
        // Text extraction is currently used only for deleted stable keys. Do
        // not inspect omitted UPDATE values if a future caller reaches here.
        Action::SQLITE_UPDATE => return Ok(None),
        _ => return Ok(None),
    };
    Ok(match value {
        ValueRef::Text(value) => Some(String::from_utf8_lossy(value).into_owned()),
        _ => None,
    })
}

fn optional_string(value: Option<String>) -> Value {
    value.map(Value::String).unwrap_or(Value::Null)
}
