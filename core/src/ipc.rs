//! Thin JSON command boundary for the replacement backend.
//!
//! This module owns no product behavior. It deserializes one command payload,
//! calls one replacement operation, and publishes that operation's committed
//! mutation receipt.

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use ts_rs::TS;

use crate::dto::FileHash;
use crate::subscription_catalog::{
    NewSubscription, NewSubscriptionQuery, SubscriptionCoverCandidateCursor,
    SubscriptionCoverSelection, SubscriptionDestinationPolicy,
};
use crate::subscriptions::gallery_dl_runner::normalize_ehentai_gallery_url;

/// Greenfield command path. Commands return `None` until their complete
/// product behavior has moved to `LibraryApplication`; production state only
/// switches after this dispatcher covers the entire command surface.
pub fn dispatch_library(
    application: &crate::library_application::LibraryApplication,
    command: &str,
    args_json: &str,
) -> Result<Option<String>, String> {
    let output = match command {
        "items.query" => {
            let input: QueryItemsInput = parse(args_json)?;
            read(application.query(&input.query, input.page)?)
        }
        "items.details" => {
            let input: RootInput = parse(args_json)?;
            read(application.details(input.root_id)?)
        }
        "items.selection_summary" => {
            let input: TargetInput = parse(args_json)?;
            read(application.selection_summary(&input.target)?)
        }
        "items.collection_note_draft" => {
            let input: TargetInput = parse(args_json)?;
            read(application.collection_note_draft(&input.target)?)
        }
        "sidebar.counts" => read(application.sidebar_counts()?),
        "library.stats" => read(application.library_statistics()?),
        "navigation.get" => read(application.navigation()?),
        "tags.list" => {
            let input: ListTagsInput = parse(args_json)?;
            read(application.list_tags(
                input.namespace.as_deref(),
                input.search.as_deref(),
                input.cursor.as_deref(),
                input.limit,
            )?)
        }
        "tags.namespace_counts" => read(application.tag_namespace_counts()?),
        "tags.get_many" => {
            let input: TagIdsInput = parse(args_json)?;
            read(application.tags_by_id(&input.tag_ids)?)
        }
        "tags.unused_count" => read(application.unused_tag_count()?),
        "duplicates.list" => {
            let input: LimitInput = parse(args_json)?;
            read(application.duplicate_candidates(input.limit)?)
        }
        "duplicates.scan" => {
            let input: ScanDuplicatesInput = parse(args_json)?;
            read(crate::duplicates::scan_library(
                application,
                input.distance_threshold,
            )?)
        }
        "subscriptions.list" => read(crate::subscription_catalog::list_library(application)?),
        "subscriptions.runs.list" => {
            let input: SubscriptionRunsInput = parse(args_json)?;
            read(crate::subscription_activity::list_runs_library(
                application,
                input.subscription_id,
                input.limit,
            )?)
        }
        "subscriptions.runs.get" => {
            let input: SubscriptionRunActivityInput = parse(args_json)?;
            read(crate::subscription_activity::run_activity_library(
                application,
                input.run_id,
                input.source_item_limit,
            )?)
        }
        "subscriptions.progress.get" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(crate::subscription_activity::current_progress_library(
                application,
                input.subscription_id,
            )?)
        }
        "subscriptions.issues.list" => {
            let input: crate::subscription_activity::IssuePageRequest = parse(args_json)?;
            read(crate::subscription_activity::list_issues_library(
                application,
                &input,
            )?)
        }
        "subscriptions.issues.acknowledge" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(
                crate::library_subscription_state::acknowledge_subscription_issues(
                    application,
                    input.subscription_id,
                )?,
            )
        }
        "subscriptions.cover.candidates" => {
            let input: SubscriptionCoverCandidatesInput = parse(args_json)?;
            read(
                crate::subscription_catalog::subscription_cover_candidates_library(
                    application,
                    input.subscription_id,
                    input.cursor.as_ref(),
                    input.limit,
                )?,
            )
        }
        "subscriptions.create" => {
            let input: NewSubscription = parse(args_json)?;
            let (subscription_id, receipt) =
                application.create_subscription_definition_library(&input, &now())?;
            read(serde_json::json!({
                "subscription_id": subscription_id,
                "receipt": receipt
            }))
        }
        "subscriptions.queries.add" => {
            let input: AddSubscriptionQueryInput = parse(args_json)?;
            let (query_id, receipt) =
                application.add_subscription_query_library(input.subscription_id, &input.query)?;
            read(serde_json::json!({"query_id": query_id, "receipt": receipt}))
        }
        "subscriptions.queries.update" => {
            let input: UpdateSubscriptionQueryInput = parse(args_json)?;
            read(application.update_subscription_query_library(input.query_id, &input.query)?)
        }
        "subscriptions.queries.run" => {
            let input: SubscriptionQueryInput = parse(args_json)?;
            let (run, receipt) =
                application.request_subscription_query_run_library(input.query_id, &now())?;
            read(serde_json::json!({
                "run_id": run.run_id,
                "created": run.created,
                "receipt": receipt
            }))
        }
        "subscriptions.queries.pause" => {
            let input: PauseSubscriptionQueryInput = parse(args_json)?;
            read(application.pause_subscription_query_library(input.query_id, input.paused)?)
        }
        "subscriptions.queries.grouping" => {
            let input: SetSubscriptionQueryGroupingInput = parse(args_json)?;
            read(
                application
                    .set_subscription_query_grouping_library(input.query_id, input.group_posts)?,
            )
        }
        "subscriptions.queries.delete" => {
            let input: SubscriptionQueryInput = parse(args_json)?;
            read(application.delete_subscription_query_library(input.query_id)?)
        }
        "subscriptions.rename" => {
            let input: RenameSubscriptionInput = parse(args_json)?;
            read(application.rename_subscription_library(input.subscription_id, &input.name)?)
        }
        "subscriptions.pause" => {
            let input: PauseSubscriptionInput = parse(args_json)?;
            read(application.pause_subscription_library(input.subscription_id, input.paused)?)
        }
        "subscriptions.pause_all" => {
            let input: PauseAllSubscriptionsInput = parse(args_json)?;
            read(application.pause_all_subscriptions_library(input.paused)?)
        }
        "subscriptions.schedule" => {
            let input: ScheduleSubscriptionInput = parse(args_json)?;
            read(application.set_subscription_schedule_library(
                input.subscription_id,
                &input.schedule,
                &now(),
            )?)
        }
        "subscriptions.posts_per_run" => {
            let input: SubscriptionPostsPerRunInput = parse(args_json)?;
            read(application.set_subscription_posts_per_run_library(
                input.subscription_id,
                input.posts_per_run,
            )?)
        }
        "subscriptions.destination" => {
            let input: SubscriptionDestinationInput = parse(args_json)?;
            read(
                application.set_subscription_destination_library(
                    input.subscription_id,
                    &input.destination,
                )?,
            )
        }
        "subscriptions.cover.set" => {
            let input: SubscriptionCoverInput = parse(args_json)?;
            read(application.set_subscription_cover_library(input.subscription_id, &input.cover)?)
        }
        "subscriptions.delete" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(application.delete_subscription_library(input.subscription_id)?)
        }
        "subscriptions.run" => {
            let input: SubscriptionInput = parse(args_json)?;
            let (run, receipt) =
                application.request_subscription_run_library(input.subscription_id, &now())?;
            read(serde_json::json!({
                "run_id": run.run_id,
                "created": run.created,
                "receipt": receipt
            }))
        }
        "subscriptions.cancel" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(application.cancel_subscription_run_library(input.subscription_id, &now())?)
        }
        "media.resolve_paths" => {
            let input: FileHashesInput = parse(args_json)?;
            read(crate::media_io::resolve_file_paths_library(
                application,
                &input.file_hashes,
            )?)
        }
        "media.formats.list" => read(crate::media_processing::formats::ACCEPTED_FORMATS),
        "media.resolve_target_paths" => {
            let input: TargetInput = parse(args_json)?;
            read(crate::media_io::resolve_target_file_paths_library(
                application,
                &input.target,
            )?)
        }
        "media.regenerate_thumbnails" => {
            let input: FileHashesInput = parse(args_json)?;
            read(crate::media_io::enqueue_thumbnail_regeneration_library(
                application,
                &input.file_hashes,
            )?)
        }
        "media.export" => read(crate::media_io::export_library(
            application,
            &parse::<crate::media_io::ExportRequest>(args_json)?,
        )?),
        "sources.list" => read(crate::auth::sources()),
        "auth.credentials.list" => read(crate::auth::list_library_credentials(application)?),
        "auth.health.list" => read(crate::auth::list_library_health(application)?),
        "auth.credentials.set" => read(crate::auth::set_library_credential(
            application,
            parse(args_json)?,
            &now(),
        )?),
        "auth.credentials.delete" => {
            let input: SiteInput = parse(args_json)?;
            read(crate::auth::delete_library_credential(
                application,
                &input.site_id,
            )?)
        }
        "items.record_view" => {
            let input: RootInput = parse(args_json)?;
            read(application.record_recent_view(input.root_id)?)
        }
        "items.clear_recent_views" => read(application.clear_recent_views()?),
        "items.set_lifecycle" => {
            let input: LifecycleInput = parse(args_json)?;
            read(application.set_lifecycle(&input.target, input.lifecycle)?)
        }
        "items.set_folder" => {
            let input: FolderMembershipInput = parse(args_json)?;
            read(application.set_folder_membership(
                &input.target,
                input.folder_id,
                input.present,
            )?)
        }
        "items.apply_tags" => {
            let input: ApplyTagsInput = parse(args_json)?;
            read(application.apply_tags(&input.target, &input.tags, input.add)?)
        }
        "items.rename" => {
            let input: RenameRootInput = parse(args_json)?;
            read(application.rename_item(input.root_id, &input.name)?)
        }
        "items.rename_many" => {
            let input: RenameRootsInput = parse(args_json)?;
            read(application.rename_items(&input.renames)?)
        }
        "items.patch_metadata" => {
            let input: PatchMetadataInput = parse(args_json)?;
            read(application.patch_metadata(&input.target, &input.patch)?)
        }
        "items.delete" => {
            let input: TargetInput = parse(args_json)?;
            read(application.delete_items(&input.target)?)
        }
        "items.organize_into_collection" => {
            let input: picto_library::OrganizeCollectionInput = parse(args_json)?;
            read(application.organize_into_collection(input)?)
        }
        "items.detach" => {
            let input: picto_library::DetachCollectionInput = parse(args_json)?;
            read(application.detach_items(input)?)
        }
        "items.ungroup" => {
            let input: CollectionInput = parse(args_json)?;
            read(application.ungroup_collection(input.collection_id)?)
        }
        "items.reorder_collection" => {
            let input: picto_library::ReorderCollectionInput = parse(args_json)?;
            read(application.reorder_collection(input)?)
        }
        "folders.create" => {
            let input: picto_library::CreateFolderInput = parse(args_json)?;
            read(application.create_folder(input)?)
        }
        "folders.rename" => {
            let input: RenameFolderInput = parse(args_json)?;
            read(application.rename_folder(input.folder_id, &input.name)?)
        }
        "folders.duplicate" => {
            let input: FolderInput = parse(args_json)?;
            read(application.duplicate_folder(input.folder_id)?)
        }
        "folders.metadata.set" => {
            let input: picto_library::FolderMetadataInput = parse(args_json)?;
            read(application.set_folder_metadata(&input)?)
        }
        "folders.auto_tags.get" => {
            let input: FolderInput = parse(args_json)?;
            read(application.folder_auto_tags(input.folder_id)?)
        }
        "folders.auto_tags.set" => {
            let input: picto_library::FolderAutoTagsInput = parse(args_json)?;
            read(application.set_folder_auto_tags(&input)?)
        }
        "folders.cover.get" => {
            let input: FolderInput = parse(args_json)?;
            read(application.folder_cover(input.folder_id)?)
        }
        "folders.cover.set" => {
            let input: picto_library::FolderCoverInput = parse(args_json)?;
            read(application.set_folder_cover(&input)?)
        }
        "folders.move" => {
            let input: MoveFolderInput = parse(args_json)?;
            read(application.move_folder(input.folder_id, input.parent_id)?)
        }
        "folders.reorder" => {
            let input: picto_library::ReorderFolderChildrenInput = parse(args_json)?;
            read(application.reorder_folder_children(&input)?)
        }
        "folders.sort_tree" => {
            let input: picto_library::SortFolderTreeInput = parse(args_json)?;
            read(application.sort_folder_tree(&input)?)
        }
        "folders.items.reorder" => {
            let input: picto_library::ReorderFolderRootsInput = parse(args_json)?;
            read(application.reorder_folder_items(&input)?)
        }
        "folders.items.sort" => {
            let input: SortFolderItemsInput = parse(args_json)?;
            read(application.sort_folder_items(input.folder_id, input.field)?)
        }
        "folders.delete" => {
            let input: FolderIdsInput = parse(args_json)?;
            read(application.delete_folders(&input.folder_ids)?)
        }
        "folders.watch.set" => {
            let input: picto_library::FolderWatchInput = parse(args_json)?;
            read(application.set_folder_watch(&input)?)
        }
        "folders.watch.clear" => {
            let input: FolderInput = parse(args_json)?;
            read(application.clear_folder_watch(input.folder_id)?)
        }
        "smart_folders.create" => {
            let input: picto_library::SmartFolderInput = parse(args_json)?;
            read(application.create_smart_folder(input)?)
        }
        "smart_folders.update" => {
            let input: UpdateSmartFolderInput = parse(args_json)?;
            read(application.update_smart_folder(input.smart_folder_id, input.value)?)
        }
        "smart_folders.move" => {
            let input: MoveSmartFolderInput = parse(args_json)?;
            read(application.move_smart_folder(input.smart_folder_id, input.parent_id)?)
        }
        "smart_folders.reorder" => {
            let input: ReorderSmartFoldersInput = parse(args_json)?;
            read(
                application
                    .reorder_smart_folder_children(input.parent_id, &input.smart_folder_ids)?,
            )
        }
        "smart_folders.delete" => {
            let input: SmartFolderInput = parse(args_json)?;
            read(application.delete_smart_folder(input.smart_folder_id)?)
        }
        "tags.rename_or_merge" => {
            let input: RenameTagInput = parse(args_json)?;
            read(application.rename_or_merge_tag(input.tag_id, &input.name)?)
        }
        "tags.delete" => {
            let input: TagInput = parse(args_json)?;
            read(application.delete_tag(input.tag_id)?)
        }
        "tags.delete_unused" => read(application.delete_unused_tags()?),
        "tags.group.create" => {
            let input: picto_library::CreateTagNamespaceInput = parse(args_json)?;
            read(application.create_tag_namespace(&input.name)?)
        }
        "tags.group.rename" => {
            let input: picto_library::RenameTagNamespaceInput = parse(args_json)?;
            read(application.rename_tag_namespace(&input)?)
        }
        "tags.group.delete" => {
            let input: picto_library::TagNamespaceInput = parse(args_json)?;
            read(application.delete_tag_namespace(input.namespace_id)?)
        }
        "duplicates.resolve" => {
            let input: ResolveDuplicateInput = parse(args_json)?;
            read(application.resolve_duplicate(input.file_id_a, input.file_id_b, input.choice)?)
        }
        "duplicates.resolve_automatically" => {
            let input: AutomaticDuplicateInput = parse(args_json)?;
            read(application.resolve_duplicate_automatically(input.file_id_a, input.file_id_b)?)
        }
        "settings.get" => read(application.application_settings()?),
        "settings.view.get" => {
            let input: ScopeInput = parse(args_json)?;
            read(application.view_preferences(&input.scope)?)
        }
        "settings.replace" => {
            let input: ValueInput = parse(args_json)?;
            read(application.replace_application_settings(&input.value)?)
        }
        "settings.patch" => {
            let input: ValueInput = parse(args_json)?;
            read(application.patch_application_settings(&input.value)?)
        }
        "settings.view.patch" => {
            let input: PatchViewSettingsInput = parse(args_json)?;
            read(application.patch_view_preferences(&input.scope, &input.value)?)
        }
        "settings.view.reset" => read(application.reset_view_preferences()?),
        "tasks.get" => read(crate::tasks::snapshot_library(application)?),
        "cloud.providers.detect" => read(crate::cloud::provider::detect_roots()),
        "cloud.status.get" => read(crate::cloud::status_library(application)?),
        "cloud.configuration.get" => read(crate::cloud::configuration_library(application)?),
        "cloud.configure" => {
            let input: crate::cloud::ConfigureCloudInput = parse(args_json)?;
            read(crate::cloud::configure_library(application, &input)?)
        }
        "cloud.pause" => {
            let input: CloudPauseInput = parse(args_json)?;
            read(crate::cloud::set_paused_library(application, input.paused)?)
        }
        "cloud.retention.update" => {
            let input: ValueInput = parse(args_json)?;
            read(crate::cloud::update_retention_library(
                application,
                &input.value,
            )?)
        }
        "history.state" => read(application.history_state()),
        "history.undo" => read(application.undo()?),
        "history.redo" => read(application.redo()?),
        _ => return Ok(None),
    }?;
    Ok(Some(output))
}

pub async fn dispatch_library_async(
    application: &crate::library_application::LibraryApplication,
    command: &str,
    args_json: &str,
) -> Result<Option<String>, String> {
    let output = match command {
        "cloud.reconcile" | "cloud.snapshot.create" => {
            let provider = crate::cloud::directory_provider_library(application)?;
            read(crate::cloud::snapshot::publish_library(application, &provider).await?)
        }
        "cloud.restore.list" => {
            let provider = crate::cloud::directory_provider_library(application)?;
            read(crate::cloud::snapshot::list_remote_for_library(application, &provider).await?)
        }
        "diagnostics.snapshot" => read(crate::diagnostics::snapshot_library(application)?),
        "ai.status" => read(crate::ai_runtime::model_status_library(application).await?),
        "ai.models.download" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models::download(application, &input.slug).await?;
            read(crate::ai_runtime::model_status_library(application).await?)
        }
        "ai.models.cancel" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models::cancel_download(application, &input.slug).await?;
            read(EmptyOutput {})
        }
        "ai.models.delete" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models::delete(application, &input.slug).await?;
            read(crate::ai_runtime::model_status_library(application).await?)
        }
        "ai.models.optimize" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models::optimize(application, &input.slug).await?;
            read(crate::ai_runtime::model_status_library(application).await?)
        }
        "ai.review.predict" => {
            let input: crate::ai_runtime::LibraryManualPredictionRequest = parse(args_json)?;
            read(crate::ai_runtime::manual_predict_library(application, input).await?)
        }
        "ai.review.unload" => {
            crate::ai_runtime::unload_library_sessions(application).await;
            read(EmptyOutput {})
        }
        "ai.review.apply" => {
            let input: AiAssignmentsInput = parse(args_json)?;
            read(
                application
                    .library()
                    .add_tag_assignments(&input.assignments)
                    .map_err(|error| error.to_string())?,
            )
        }
        "pixiv_oauth_start" => read(crate::subscriptions::pixiv_oauth::generate_challenge()),
        "pixiv_oauth_exchange" => {
            let input: PixivOAuthExchangeInput = parse(args_json)?;
            let refresh_token =
                crate::subscriptions::pixiv_oauth::exchange_code(&input.code, &input.code_verifier)
                    .await?;
            let cookies = input
                .phpsessid
                .filter(|value| !value.trim().is_empty())
                .map(|value| std::collections::HashMap::from([("PHPSESSID".to_string(), value)]));
            crate::auth::set_library_credential(
                application,
                crate::auth::SetCredentialInput {
                    site_id: "pixiv".into(),
                    credential_type: "oauth_token".into(),
                    display_name: Some("Pixiv".into()),
                    username: None,
                    password: None,
                    cookies,
                    headers: None,
                    oauth_token: Some(refresh_token),
                },
                &now(),
            )?;
            read(PixivOAuthExchangeOutput { ok: true })
        }
        "subscriptions.gallery.start" => {
            let input: GalleryImportInput = parse(args_json)?;
            let url = normalize_ehentai_gallery_url(&input.url)?;
            let is_exhentai = url.starts_with("https://exhentai.org/");
            if input.service_id.as_deref().is_some_and(|service| {
                !matches!(
                    (service, is_exhentai),
                    ("ehentai", false) | ("exhentai", true)
                )
            }) {
                return Err("The selected gallery service does not match the URL".into());
            }
            let gallery_id = url
                .split('/')
                .nth(4)
                .ok_or_else(|| "E-Hentai gallery URL has no gallery ID".to_string())?;
            let definition = NewSubscription {
                name: format!(
                    "{} Gallery {gallery_id}",
                    if is_exhentai { "ExHentai" } else { "E-Hentai" }
                ),
                schedule: "manual".into(),
                initial_post_limit: None,
                periodic_post_limit: None,
                queries: vec![NewSubscriptionQuery {
                    site_id: "ehentai".into(),
                    query_text: url,
                    display_name: Some("Gallery import".into()),
                    notes: None,
                    group_posts: true,
                }],
            };
            let timestamp = now();
            let (subscription_id, _) =
                application.create_subscription_definition_library(&definition, &timestamp)?;
            if let Err(error) =
                crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
                    application.root(),
                    subscription_id,
                )
                .await
            {
                let _ = application.delete_subscription_library(subscription_id);
                return Err(error);
            }
            let (run, receipt) = application
                .request_subscription_run_library(subscription_id, &timestamp)
                .inspect_err(|_| {
                    let _ = application.delete_subscription_library(subscription_id);
                })?;
            read(CreatedSubscriptionRun {
                run_id: run.run_id,
                created: run.created,
                receipt,
            })
        }
        "subscriptions.gallery.cleanup" => {
            let input: SubscriptionInput = parse(args_json)?;
            let catalog = crate::subscription_catalog::list_library(application)?;
            let Some(subscription) = catalog
                .subscriptions
                .iter()
                .find(|entry| entry.subscription_id == input.subscription_id)
            else {
                return read(Option::<GalleryImportCleanupResult>::None).map(Some);
            };
            if subscription.queries.len() != 1 || subscription.queries[0].site_id != "ehentai" {
                return Err("Only transient E-Hentai gallery jobs can use gallery cleanup".into());
            }
            crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
                application.root(),
                input.subscription_id,
            )
            .await?;
            let title = application
                .library()
                .auxiliary_read(
                    picto_library::database::WorkPriority::VisibleRead,
                    |connection| {
                        connection
                            .query_row(
                                "SELECT NULLIF(TRIM(post.title), '')
                                 FROM subscription_source_post link
                                 JOIN source_post post ON post.source_post_id = link.source_post_id
                                 WHERE link.subscription_id = ?1 AND post.root_item_id IS NOT NULL
                                 ORDER BY post.updated_at DESC, post.source_post_id DESC LIMIT 1",
                                [input.subscription_id],
                                |row| row.get::<_, Option<String>>(0),
                            )
                            .optional()
                            .map(Option::flatten)
                            .map_err(Into::into)
                    },
                )
                .map_err(|error| error.to_string())?;
            match application.delete_subscription_library(input.subscription_id) {
                Ok(receipt) => read(GalleryImportCleanupResult { title, receipt }),
                Err(error) if error.contains("subscription does not exist") => {
                    read(Option::<GalleryImportCleanupResult>::None)
                }
                Err(error) => Err(error),
            }
        }
        "media.request_thumbnail" => {
            let input: FileHashInput = parse(args_json)?;
            read(crate::media_io::request_thumbnail_library(
                application,
                &input.file_hash,
            )?)
        }
        "media.render_thumbnail_now" => {
            let input: FileHashInput = parse(args_json)?;
            read(
                crate::media_io::render_thumbnail_now_library(application, &input.file_hash)
                    .await?,
            )
        }
        "imports.enqueue" => {
            let input: crate::library_import::ManualImportInput = parse(args_json)?;
            read(crate::library_import::enqueue_manual_import(application, &input).await?)
        }
        "imports.folder.analyze" => {
            let input: crate::library_import::FolderTreeAnalysisInput = parse(args_json)?;
            read(crate::library_import::analyze_folder_tree(
                application,
                &input,
            )?)
        }
        "subscriptions.reset" => {
            let input: SubscriptionInput = parse(args_json)?;
            read(
                application
                    .reset_subscription_library(input.subscription_id)
                    .await?,
            )
        }
        _ => return dispatch_library(application, command, args_json),
    }?;
    Ok(Some(output))
}

#[derive(Deserialize)]
struct PixivOAuthExchangeInput {
    code: String,
    code_verifier: String,
    phpsessid: Option<String>,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudPauseInput {
    paused: bool,
}

#[derive(Serialize)]
struct PixivOAuthExchangeOutput {
    ok: bool,
}

fn parse<T: DeserializeOwned>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|error| format!("Invalid command arguments: {error}"))
}

fn read<T: Serialize>(value: T) -> Result<String, String> {
    serde_json::to_string(&value)
        .map_err(|error| format!("Failed to serialize command result: {error}"))
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Deserialize)]
struct QueryItemsInput {
    query: picto_library::query::RootQuery,
    page: picto_library::query::PageRequest,
}

#[derive(Deserialize)]
struct RootInput {
    root_id: picto_library::RootId,
}

#[derive(Deserialize)]
struct TargetInput {
    target: picto_library::selection::SelectionTarget,
}

#[derive(Deserialize)]
struct TagIdsInput {
    tag_ids: Vec<picto_library::TagId>,
}

#[derive(Deserialize)]
struct LifecycleInput {
    target: picto_library::selection::SelectionTarget,
    lifecycle: picto_library::Lifecycle,
}

#[derive(Deserialize)]
struct FolderMembershipInput {
    target: picto_library::selection::SelectionTarget,
    folder_id: picto_library::FolderId,
    present: bool,
}

#[derive(Deserialize)]
struct ApplyTagsInput {
    target: picto_library::selection::SelectionTarget,
    tags: Vec<String>,
    add: bool,
}

#[derive(Deserialize)]
struct RenameRootInput {
    root_id: picto_library::RootId,
    name: String,
}

#[derive(Deserialize)]
struct RenameRootsInput {
    renames: Vec<picto_library::RootRename>,
}

#[derive(Deserialize)]
struct PatchMetadataInput {
    target: picto_library::selection::SelectionTarget,
    patch: picto_library::RootMetadataPatch,
}

#[derive(Deserialize)]
struct CollectionInput {
    collection_id: picto_library::RootId,
}

#[derive(Deserialize)]
struct FolderInput {
    folder_id: picto_library::FolderId,
}

#[derive(Deserialize)]
struct SortFolderItemsInput {
    folder_id: picto_library::FolderId,
    field: picto_library::ContentSortField,
}

#[derive(Deserialize)]
struct RenameFolderInput {
    folder_id: picto_library::FolderId,
    name: String,
}

#[derive(Deserialize)]
struct MoveFolderInput {
    folder_id: picto_library::FolderId,
    parent_id: Option<picto_library::FolderId>,
}

#[derive(Deserialize)]
struct FolderIdsInput {
    folder_ids: Vec<picto_library::FolderId>,
}

#[derive(Deserialize)]
struct TagInput {
    tag_id: picto_library::TagId,
}

#[derive(Deserialize)]
struct RenameTagInput {
    tag_id: picto_library::TagId,
    name: String,
}

#[derive(Deserialize)]
struct ResolveDuplicateInput {
    file_id_a: picto_library::FileId,
    file_id_b: picto_library::FileId,
    choice: picto_library::DuplicateResolutionChoice,
}

#[derive(Deserialize)]
struct AutomaticDuplicateInput {
    file_id_a: picto_library::FileId,
    file_id_b: picto_library::FileId,
}

#[derive(Deserialize)]
struct AiAssignmentsInput {
    assignments: Vec<picto_library::RootTagAssignment>,
}

#[derive(Serialize)]
struct CreatedSubscriptionRun {
    run_id: i64,
    created: bool,
    receipt: picto_library::MutationReceipt,
}

#[derive(Serialize)]
struct GalleryImportCleanupResult {
    title: Option<String>,
    receipt: picto_library::MutationReceipt,
}

#[derive(Deserialize)]
struct UpdateSmartFolderInput {
    smart_folder_id: picto_library::SmartFolderId,
    value: picto_library::SmartFolderInput,
}

#[derive(Deserialize)]
struct SmartFolderInput {
    smart_folder_id: picto_library::SmartFolderId,
}

#[derive(Deserialize)]
struct MoveSmartFolderInput {
    smart_folder_id: picto_library::SmartFolderId,
    parent_id: Option<picto_library::SmartFolderId>,
}

#[derive(Deserialize)]
struct ReorderSmartFoldersInput {
    parent_id: Option<picto_library::SmartFolderId>,
    smart_folder_ids: Vec<picto_library::SmartFolderId>,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FileHashInput {
    file_hash: FileHash,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FileHashesInput {
    file_hashes: Vec<FileHash>,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ListTagsInput {
    namespace: Option<String>,
    search: Option<String>,
    cursor: Option<String>,
    #[serde(default = "default_limit")]
    #[ts(type = "number")]
    limit: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LimitInput {
    #[serde(default = "default_limit")]
    #[ts(type = "number")]
    limit: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ScanDuplicatesInput {
    #[serde(default = "default_distance_threshold")]
    #[ts(type = "number")]
    distance_threshold: u32,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AddSubscriptionQueryInput {
    #[ts(type = "number")]
    subscription_id: i64,
    query: NewSubscriptionQuery,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct GalleryImportInput {
    service_id: Option<String>,
    url: String,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct UpdateSubscriptionQueryInput {
    #[ts(type = "number")]
    query_id: i64,
    query: NewSubscriptionQuery,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PauseSubscriptionQueryInput {
    #[ts(type = "number")]
    query_id: i64,
    paused: bool,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SetSubscriptionQueryGroupingInput {
    #[ts(type = "number")]
    query_id: i64,
    group_posts: bool,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionQueryInput {
    #[ts(type = "number")]
    query_id: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionInput {
    #[ts(type = "number")]
    subscription_id: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverCandidatesInput {
    #[ts(type = "number")]
    subscription_id: i64,
    cursor: Option<SubscriptionCoverCandidateCursor>,
    #[serde(default = "default_cover_candidate_limit")]
    #[ts(type = "number")]
    limit: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionRunsInput {
    #[ts(type = "number")]
    subscription_id: i64,
    #[serde(default = "default_page_limit")]
    #[ts(type = "number")]
    limit: usize,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionRunActivityInput {
    #[ts(type = "number")]
    run_id: i64,
    #[serde(default = "default_page_limit")]
    #[ts(type = "number")]
    source_item_limit: usize,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RenameSubscriptionInput {
    #[ts(type = "number")]
    subscription_id: i64,
    name: String,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PauseSubscriptionInput {
    #[ts(type = "number")]
    subscription_id: i64,
    paused: bool,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PauseAllSubscriptionsInput {
    paused: bool,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ScheduleSubscriptionInput {
    #[ts(type = "number")]
    subscription_id: i64,
    schedule: String,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionPostsPerRunInput {
    #[ts(type = "number")]
    subscription_id: i64,
    #[ts(type = "number")]
    posts_per_run: i64,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionDestinationInput {
    #[ts(type = "number")]
    subscription_id: i64,
    destination: SubscriptionDestinationPolicy,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SubscriptionCoverInput {
    #[ts(type = "number")]
    subscription_id: i64,
    cover: SubscriptionCoverSelection,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ScopeInput {
    scope: String,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SiteInput {
    site_id: String,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ValueInput {
    #[ts(type = "unknown")]
    value: serde_json::Value,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PatchViewSettingsInput {
    scope: String,
    #[ts(type = "unknown")]
    value: serde_json::Value,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ModelInput {
    slug: String,
}

#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct EmptyOutput {}

fn default_limit() -> i64 {
    100
}

fn default_cover_candidate_limit() -> i64 {
    200
}

fn default_page_limit() -> usize {
    100
}

fn default_distance_threshold() -> u32 {
    crate::duplicates::DEFAULT_GLOBAL_DISTANCE_THRESHOLD
}
