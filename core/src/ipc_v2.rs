//! Thin JSON command boundary for the replacement backend.
//!
//! This module owns no product behavior. It deserializes one command payload,
//! calls one replacement operation, and publishes that operation's committed
//! mutation receipt.

use chrono::Utc;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::app::{
    Application, FileHash, ItemId, ItemQuery, ItemTarget, Lifecycle, MutationReceipt,
};
use crate::duplicates_v2::{DuplicateCandidate, ResolutionChoice};
use crate::folders_v2::{FolderId, FolderMutationReceipt, FolderWatchInput};
use crate::navigation_v2::{CreateSmartFolderInput, SmartFolderMutationReceipt};
use crate::operations_v2::MediaMetadataPatch;
use crate::query_v2::ItemPageRequest;
use crate::subscription_catalog_v2::{NewSubscription, NewSubscriptionQuery};

pub fn dispatch(
    application: &Application,
    command: &str,
    args_json: &str,
) -> Result<String, String> {
    match command {
        "items.query" => {
            let input: QueryItemsInput = parse(args_json)?;
            read(crate::query_v2::query(
                application.store(),
                &input.query,
                input.page,
            )?)
        }
        "items.details" => {
            let input: ItemInput = parse(args_json)?;
            read(crate::query_v2::details(
                application.store(),
                input.item_id,
            )?)
        }
        "items.selection_summary" => {
            let input: TargetInput = parse(args_json)?;
            read(crate::query_v2::selection_summary(
                application.store(),
                &input.target,
            )?)
        }
        "sidebar.counts" => read(crate::query_v2::sidebar_counts(application.store())?),
        "navigation.get" => read(crate::navigation_v2::navigation(application)?),
        "tags.list" => {
            let input: ListTagsInput = parse(args_json)?;
            read(crate::tags_v2::list(
                application,
                input.namespace.as_deref(),
                input.search.as_deref(),
                input.cursor.as_deref(),
                input.limit,
            )?)
        }
        "tags.namespace_counts" => read(crate::tags_v2::namespace_counts(application)?),
        "tags.relations" => {
            let input: TagInput = parse(args_json)?;
            read(crate::tags_v2::relations(application, input.tag_id)?)
        }
        "duplicates.list" => {
            let input: LimitInput = parse(args_json)?;
            read(crate::duplicates_v2::list_candidates(
                application,
                input.limit,
            )?)
        }
        "subscriptions.list" => read(crate::subscription_catalog_v2::list(application)?),
        "subscriptions.runs.list" => {
            let input: SubscriptionRunsInput = parse(args_json)?;
            read(crate::subscription_activity_v2::list_runs(
                application.store(),
                input.subscription_id,
                input.limit,
            )?)
        }
        "subscriptions.runs.get" => {
            let input: SubscriptionRunActivityInput = parse(args_json)?;
            read(crate::subscription_activity_v2::run_activity(
                application.store(),
                input.run_id,
                input.source_item_limit,
            )?)
        }
        "subscriptions.progress.get" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(crate::subscription_activity_v2::current_progress(
                application.store(),
                input.subscription_id,
            )?)
        }
        "subscriptions.issues.list" => {
            let input: crate::subscription_activity_v2::IssuePageRequest = parse(args_json)?;
            read(crate::subscription_activity_v2::list_issues(
                application.store(),
                &input,
            )?)
        }
        "sources.list" => read(crate::auth_v2::sources()),
        "auth.credentials.list" => read(crate::auth_v2::list_credentials(application.store())?),
        "auth.health.list" => read(crate::auth_v2::list_health(application.store())?),
        "settings.get" => read(crate::settings_v2::application_settings(application)?),
        "settings.view.get" => {
            let input: ScopeInput = parse(args_json)?;
            read(crate::settings_v2::view_preferences(
                application,
                &input.scope,
            )?)
        }
        "tasks.get" => read(crate::tasks_v2::snapshot(application)?),
        "media.resolve_paths" => {
            let input: FileHashesInput = parse(args_json)?;
            read(crate::media_io_v2::resolve_file_paths(
                application.store(),
                application.blobs(),
                &input.file_hashes,
            )?)
        }
        "media.regenerate_thumbnails" => {
            let input: FileHashesInput = parse(args_json)?;
            let output = crate::media_io_v2::enqueue_thumbnail_regeneration(
                application,
                &input.file_hashes,
            )?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "media.export" => {
            let input: crate::media_io_v2::ExportRequest = parse(args_json)?;
            read(crate::media_io_v2::export(
                application.store(),
                application.blobs(),
                application.store().library_root(),
                &input,
            )?)
        }
        "items.record_view" => {
            let input: ItemInput = parse(args_json)?;
            publish(application, application.record_recent_view(input.item_id)?)
        }

        "items.set_lifecycle" => {
            let input: LifecycleInput = parse(args_json)?;
            publish(
                application,
                application.set_lifecycle(&input.target, input.lifecycle)?,
            )
        }
        "items.set_folder" => {
            let input: FolderMembershipInput = parse(args_json)?;
            publish(
                application,
                application.set_folder_membership(&input.target, input.folder_id, input.present)?,
            )
        }
        "items.group" => {
            let output = application.group_items(parse(args_json)?)?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "items.detach" => publish(application, application.detach_items(parse(args_json)?)?),
        "items.ungroup" => {
            let input: ItemInput = parse(args_json)?;
            publish(application, application.ungroup_collection(input.item_id)?)
        }
        "items.reorder_collection" => publish(
            application,
            application.reorder_collection(parse(args_json)?)?,
        ),
        "items.set_collection_cover" => {
            let input: CollectionCoverInput = parse(args_json)?;
            publish(
                application,
                application.set_collection_cover(input.collection_id, input.media_item_id)?,
            )
        }
        "items.apply_tags" => {
            let input: ApplyTagsInput = parse(args_json)?;
            publish(
                application,
                application.apply_tags(
                    &input.target,
                    &input.tags,
                    input.add,
                    input.provenance_mask,
                )?,
            )
        }
        "items.patch_metadata" => {
            let input: PatchMetadataInput = parse(args_json)?;
            publish(
                application,
                application.patch_metadata(&input.target, &input.patch)?,
            )
        }
        "items.delete" => {
            let input: TargetInput = parse(args_json)?;
            let output = application.delete_items(&input.target)?;
            publish_nested(application, output, |output| &output.receipt)
        }

        "folders.create" => {
            let (folder_id, receipt) = application.create_folder(&parse(args_json)?)?;
            publish_folder(application, CreatedFolder { folder_id, receipt })
        }
        "folders.rename" => {
            let input: RenameFolderInput = parse(args_json)?;
            publish_folder(
                application,
                application.rename_folder(input.folder_id, &input.name)?,
            )
        }
        "folders.move" => {
            let input: MoveFolderInput = parse(args_json)?;
            publish_folder(
                application,
                application.move_folder(input.folder_id, input.parent_id)?,
            )
        }
        "folders.reorder" => publish_folder(
            application,
            application.reorder_folder_children(&parse(args_json)?)?,
        ),
        "folders.delete" => {
            let input: FolderInput = parse(args_json)?;
            publish_folder(application, application.delete_folder(input.folder_id)?)
        }
        "folders.watch.set" => {
            let input: FolderWatchInput = parse(args_json)?;
            publish_folder(application, application.set_folder_watch(&input)?)
        }
        "folders.watch.clear" => {
            let input: FolderInput = parse(args_json)?;
            publish_folder(
                application,
                application.clear_folder_watch(input.folder_id)?,
            )
        }

        "smart_folders.create" => {
            let (smart_folder_id, receipt) =
                application.create_smart_folder_v2(&parse(args_json)?)?;
            publish_smart(
                application,
                CreatedSmartFolder {
                    smart_folder_id,
                    receipt,
                },
            )
        }
        "smart_folders.update" => {
            let input: UpdateSmartFolderInput = parse(args_json)?;
            publish_smart(
                application,
                application.update_smart_folder_v2(input.smart_folder_id, &input.value)?,
            )
        }
        "smart_folders.move" => {
            let input: MoveSmartFolderInput = parse(args_json)?;
            publish_smart(
                application,
                application.move_smart_folder_v2(input.smart_folder_id, input.parent_id)?,
            )
        }
        "smart_folders.reorder" => {
            let input: ReorderSmartFoldersInput = parse(args_json)?;
            publish_smart(
                application,
                application
                    .reorder_smart_folder_children_v2(input.parent_id, &input.smart_folder_ids)?,
            )
        }
        "smart_folders.delete" => {
            let input: SmartFolderInput = parse(args_json)?;
            publish_smart(
                application,
                application.delete_smart_folder_v2(input.smart_folder_id)?,
            )
        }

        "tags.set_alias" => {
            let input: TagAliasInput = parse(args_json)?;
            publish(
                application,
                application.set_tag_alias(input.from_tag_id, input.to_tag_id)?,
            )
        }
        "tags.set_implication" => {
            let input: TagImplicationInput = parse(args_json)?;
            publish(
                application,
                application.set_tag_implication(
                    input.child_tag_id,
                    input.parent_tag_id,
                    input.present,
                )?,
            )
        }
        "tags.rename_or_merge" => {
            let input: RenameTagInput = parse(args_json)?;
            publish(
                application,
                application.rename_or_merge_tag(input.tag_id, &input.name)?,
            )
        }
        "tags.delete" => {
            let input: TagInput = parse(args_json)?;
            publish(application, application.delete_tag(input.tag_id)?)
        }

        "duplicates.scan" => {
            let input: ScanDuplicatesInput = parse(args_json)?;
            let output = crate::duplicates_v2::scan(application, input.distance_threshold)?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "duplicates.resolve" => {
            let input: ResolveDuplicateInput = parse(args_json)?;
            let output = crate::duplicates_v2::resolve(
                application,
                input.file_id_a,
                input.file_id_b,
                input.choice,
            )?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "duplicates.resolve_automatically" => {
            let input: AutomaticDuplicateInput = parse(args_json)?;
            let output =
                crate::duplicates_v2::resolve_automatically(application, &input.candidate)?;
            publish_nested(application, output, |output| &output.receipt)
        }

        "subscriptions.create" => {
            let input: NewSubscription = parse(args_json)?;
            let (subscription_id, receipt) =
                application.create_subscription_definition(&input, &now())?;
            publish(
                application,
                CreatedSubscription {
                    subscription_id,
                    receipt,
                },
            )
        }
        "subscriptions.queries.add" => {
            let input: AddSubscriptionQueryInput = parse(args_json)?;
            let (query_id, receipt) =
                application.add_subscription_query(input.subscription_id, &input.query)?;
            publish(application, CreatedSubscriptionQuery { query_id, receipt })
        }
        "subscriptions.queries.update" => {
            let input: UpdateSubscriptionQueryInput = parse(args_json)?;
            publish(
                application,
                application.update_subscription_query(input.query_id, &input.query)?,
            )
        }
        "subscriptions.queries.pause" => {
            let input: PauseSubscriptionQueryInput = parse(args_json)?;
            publish(
                application,
                application.pause_subscription_query(input.query_id, input.paused)?,
            )
        }
        "subscriptions.queries.delete" => {
            let input: SubscriptionQueryInput = parse(args_json)?;
            publish(
                application,
                application.delete_subscription_query(input.query_id)?,
            )
        }
        "subscriptions.rename" => {
            let input: RenameSubscriptionInput = parse(args_json)?;
            publish(
                application,
                application.rename_subscription(input.subscription_id, &input.name)?,
            )
        }
        "subscriptions.pause" => {
            let input: PauseSubscriptionInput = parse(args_json)?;
            publish(
                application,
                application.pause_subscription(input.subscription_id, input.paused)?,
            )
        }
        "subscriptions.schedule" => {
            let input: ScheduleSubscriptionInput = parse(args_json)?;
            publish(
                application,
                application.set_subscription_schedule(
                    input.subscription_id,
                    &input.schedule,
                    &now(),
                )?,
            )
        }
        "subscriptions.delete" => {
            let input: SubscriptionInput = parse(args_json)?;
            publish(
                application,
                application.delete_subscription(input.subscription_id)?,
            )
        }
        "subscriptions.run" => {
            let input: SubscriptionInput = parse(args_json)?;
            let (run, receipt) =
                application.request_subscription_run(input.subscription_id, &now())?;
            publish(
                application,
                CreatedSubscriptionRun {
                    run_id: run.run_id,
                    created: run.created,
                    receipt,
                },
            )
        }
        "subscriptions.cancel" => {
            let input: SubscriptionInput = parse(args_json)?;
            publish(
                application,
                application.cancel_subscription_run(input.subscription_id, &now())?,
            )
        }
        "auth.credentials.set" => publish(
            application,
            crate::auth_v2::set_credential(application, parse(args_json)?, &now())?,
        ),
        "auth.credentials.delete" => {
            let input: SiteInput = parse(args_json)?;
            publish(
                application,
                crate::auth_v2::delete_credential(application, &input.site_id)?,
            )
        }

        "settings.replace" => {
            let input: ValueInput = parse(args_json)?;
            publish(
                application,
                application.replace_application_settings(&input.value)?,
            )
        }
        "settings.patch" => {
            let input: ValueInput = parse(args_json)?;
            publish(
                application,
                application.patch_application_settings(&input.value)?,
            )
        }
        "settings.view.patch" => {
            let input: PatchViewSettingsInput = parse(args_json)?;
            publish(
                application,
                application.patch_view_preferences(&input.scope, &input.value)?,
            )
        }
        _ => Err(format!("Unknown replacement command: {command}")),
    }
}

pub async fn dispatch_async(
    application: &Application,
    command: &str,
    args_json: &str,
) -> Result<String, String> {
    if command == "media.ensure_thumbnail" {
        let input: FileHashInput = parse(args_json)?;
        return read(
            crate::media_io_v2::ensure_thumbnail(
                application.store(),
                application.blobs(),
                &input.file_hash,
            )
            .await?,
        );
    }
    if command == "imports.enqueue" {
        let input: crate::import_v2::ManualImportInput = parse(args_json)?;
        return read(application.enqueue_manual_import(&input).await?);
    }
    dispatch(application, command, args_json)
}

fn parse<T: DeserializeOwned>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|error| format!("Invalid command arguments: {error}"))
}

fn read<T: Serialize>(value: T) -> Result<String, String> {
    serde_json::to_string(&value)
        .map_err(|error| format!("Failed to serialize command result: {error}"))
}

fn publish<T: Serialize + Receipt>(application: &Application, value: T) -> Result<String, String> {
    application.publish(value.receipt());
    read(value)
}

fn publish_nested<T: Serialize>(
    application: &Application,
    value: T,
    receipt: impl FnOnce(&T) -> &MutationReceipt,
) -> Result<String, String> {
    application.publish(receipt(&value));
    read(value)
}

fn publish_folder<T: Serialize + FolderReceipt>(
    application: &Application,
    value: T,
) -> Result<String, String> {
    application.publish(&value.folder_receipt().receipt);
    read(value)
}

fn publish_smart<T: Serialize + SmartReceipt>(
    application: &Application,
    value: T,
) -> Result<String, String> {
    application.publish(&value.smart_receipt().receipt);
    read(value)
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

trait Receipt {
    fn receipt(&self) -> &MutationReceipt;
}

impl Receipt for MutationReceipt {
    fn receipt(&self) -> &MutationReceipt {
        self
    }
}

macro_rules! receipt_field {
    ($type:ty) => {
        impl Receipt for $type {
            fn receipt(&self) -> &MutationReceipt {
                &self.receipt
            }
        }
    };
}

trait FolderReceipt {
    fn folder_receipt(&self) -> &FolderMutationReceipt;
}
impl FolderReceipt for FolderMutationReceipt {
    fn folder_receipt(&self) -> &FolderMutationReceipt {
        self
    }
}

trait SmartReceipt {
    fn smart_receipt(&self) -> &SmartFolderMutationReceipt;
}
impl SmartReceipt for SmartFolderMutationReceipt {
    fn smart_receipt(&self) -> &SmartFolderMutationReceipt {
        self
    }
}

#[derive(Serialize)]
struct CreatedFolder {
    folder_id: FolderId,
    receipt: FolderMutationReceipt,
}
impl FolderReceipt for CreatedFolder {
    fn folder_receipt(&self) -> &FolderMutationReceipt {
        &self.receipt
    }
}

#[derive(Serialize)]
struct CreatedSmartFolder {
    smart_folder_id: i64,
    receipt: SmartFolderMutationReceipt,
}
impl SmartReceipt for CreatedSmartFolder {
    fn smart_receipt(&self) -> &SmartFolderMutationReceipt {
        &self.receipt
    }
}

#[derive(Serialize)]
struct CreatedSubscription {
    subscription_id: i64,
    receipt: MutationReceipt,
}
#[derive(Serialize)]
struct CreatedSubscriptionQuery {
    query_id: i64,
    receipt: MutationReceipt,
}
#[derive(Serialize)]
struct CreatedSubscriptionRun {
    run_id: i64,
    created: bool,
    receipt: MutationReceipt,
}
receipt_field!(CreatedSubscription);
receipt_field!(CreatedSubscriptionQuery);
receipt_field!(CreatedSubscriptionRun);

#[derive(Deserialize)]
struct QueryItemsInput {
    query: ItemQuery,
    page: ItemPageRequest,
}
#[derive(Deserialize)]
struct ItemInput {
    item_id: ItemId,
}
#[derive(Deserialize)]
struct FileHashInput {
    file_hash: FileHash,
}
#[derive(Deserialize)]
struct FileHashesInput {
    file_hashes: Vec<FileHash>,
}
#[derive(Deserialize)]
struct TargetInput {
    target: ItemTarget,
}
#[derive(Deserialize)]
struct LifecycleInput {
    target: ItemTarget,
    lifecycle: Lifecycle,
}
#[derive(Deserialize)]
struct FolderMembershipInput {
    target: ItemTarget,
    folder_id: i64,
    present: bool,
}
#[derive(Deserialize)]
struct CollectionCoverInput {
    collection_id: ItemId,
    media_item_id: ItemId,
}
#[derive(Deserialize)]
struct ApplyTagsInput {
    target: ItemTarget,
    tags: Vec<String>,
    add: bool,
    #[serde(default = "default_provenance_mask")]
    provenance_mask: i64,
}
#[derive(Deserialize)]
struct PatchMetadataInput {
    target: ItemTarget,
    patch: MediaMetadataPatch,
}
#[derive(Deserialize)]
struct ListTagsInput {
    namespace: Option<String>,
    search: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}
#[derive(Deserialize)]
struct TagInput {
    tag_id: i64,
}
#[derive(Deserialize)]
struct TagAliasInput {
    from_tag_id: i64,
    to_tag_id: Option<i64>,
}
#[derive(Deserialize)]
struct TagImplicationInput {
    child_tag_id: i64,
    parent_tag_id: i64,
    present: bool,
}
#[derive(Deserialize)]
struct RenameTagInput {
    tag_id: i64,
    name: String,
}
#[derive(Deserialize)]
struct LimitInput {
    #[serde(default = "default_limit")]
    limit: i64,
}
#[derive(Deserialize)]
struct ScanDuplicatesInput {
    #[serde(default = "default_distance_threshold")]
    distance_threshold: u32,
}
#[derive(Deserialize)]
struct ResolveDuplicateInput {
    file_id_a: i64,
    file_id_b: i64,
    choice: ResolutionChoice,
}
#[derive(Deserialize)]
struct AutomaticDuplicateInput {
    candidate: DuplicateCandidate,
}
#[derive(Deserialize)]
struct FolderInput {
    folder_id: FolderId,
}
#[derive(Deserialize)]
struct RenameFolderInput {
    folder_id: FolderId,
    name: String,
}
#[derive(Deserialize)]
struct MoveFolderInput {
    folder_id: FolderId,
    parent_id: Option<FolderId>,
}
#[derive(Deserialize)]
struct SmartFolderInput {
    smart_folder_id: i64,
}
#[derive(Deserialize)]
struct UpdateSmartFolderInput {
    smart_folder_id: i64,
    value: CreateSmartFolderInput,
}
#[derive(Deserialize)]
struct MoveSmartFolderInput {
    smart_folder_id: i64,
    parent_id: Option<i64>,
}
#[derive(Deserialize)]
struct ReorderSmartFoldersInput {
    parent_id: Option<i64>,
    smart_folder_ids: Vec<i64>,
}
#[derive(Deserialize)]
struct AddSubscriptionQueryInput {
    subscription_id: i64,
    query: NewSubscriptionQuery,
}
#[derive(Deserialize)]
struct UpdateSubscriptionQueryInput {
    query_id: i64,
    query: NewSubscriptionQuery,
}
#[derive(Deserialize)]
struct PauseSubscriptionQueryInput {
    query_id: i64,
    paused: bool,
}
#[derive(Deserialize)]
struct SubscriptionQueryInput {
    query_id: i64,
}
#[derive(Deserialize)]
struct SubscriptionInput {
    subscription_id: i64,
}
#[derive(Deserialize)]
struct SubscriptionRunsInput {
    subscription_id: i64,
    #[serde(default = "default_page_limit")]
    limit: usize,
}
#[derive(Deserialize)]
struct SubscriptionRunActivityInput {
    run_id: i64,
    #[serde(default = "default_page_limit")]
    source_item_limit: usize,
}
#[derive(Deserialize)]
struct RenameSubscriptionInput {
    subscription_id: i64,
    name: String,
}
#[derive(Deserialize)]
struct PauseSubscriptionInput {
    subscription_id: i64,
    paused: bool,
}
#[derive(Deserialize)]
struct ScheduleSubscriptionInput {
    subscription_id: i64,
    schedule: String,
}
#[derive(Deserialize)]
struct ScopeInput {
    scope: String,
}

#[derive(Deserialize)]
struct SiteInput {
    site_id: String,
}
#[derive(Deserialize)]
struct ValueInput {
    value: serde_json::Value,
}
#[derive(Deserialize)]
struct PatchViewSettingsInput {
    scope: String,
    value: serde_json::Value,
}

fn default_limit() -> i64 {
    100
}
fn default_page_limit() -> usize {
    100
}
fn default_distance_threshold() -> u32 {
    10
}
fn default_provenance_mask() -> i64 {
    1
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rusqlite::params;

    use super::*;
    use crate::query_v2::ItemPage;
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Application, ItemId) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let (item_id, _) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('ipc-hash', 'image/png', 10, 'now')",
                    [],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('ipc-item', 'media', 'now', 'now')",
                    [],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, 'now', 'now')",
                    params![item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle)
                     VALUES (?1, 'active')",
                    [item_id],
                )?;
                Ok(item_id)
            })
            .unwrap();
        (directory, Application::new(store), ItemId(item_id))
    }

    fn page(application: &Application, scope: &str) -> ItemPage {
        let output = dispatch(
            application,
            "items.query",
            &format!(
                r#"{{"query":{{"scope":{{"kind":"{scope}"}}}},"page":{{"offset":0,"limit":50}}}}"#
            ),
        )
        .unwrap();
        serde_json::from_str(&output).unwrap()
    }

    #[test]
    fn dispatch_uses_canonical_scope_and_reconciles_after_mutation() {
        let (_directory, application, item_id) = fixture();
        assert_eq!(page(&application, "all").visible_item_count, 1);
        assert_eq!(page(&application, "inbox").visible_item_count, 0);

        let output = dispatch(
            &application,
            "items.set_lifecycle",
            &format!(
                r#"{{"target":{{"kind":"explicit","item_ids":[{}]}},"lifecycle":"inbox"}}"#,
                item_id.0
            ),
        )
        .unwrap();
        let receipt: MutationReceipt = serde_json::from_str(&output).unwrap();
        assert_eq!(receipt.item_ids, vec![item_id]);
        assert!(receipt
            .resources
            .iter()
            .any(|resource| resource == "library"));
        assert_eq!(page(&application, "all").visible_item_count, 0);
        assert_eq!(page(&application, "inbox").visible_item_count, 1);
    }

    #[test]
    fn dispatch_rejects_unknown_commands_and_invalid_arguments() {
        let (_directory, application, _) = fixture();
        assert_eq!(
            dispatch(&application, "legacy.magic", "{}").unwrap_err(),
            "Unknown replacement command: legacy.magic"
        );
        assert!(dispatch(&application, "items.details", "{}")
            .unwrap_err()
            .starts_with("Invalid command arguments:"));
    }
}
