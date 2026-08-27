//! Thin JSON command boundary for the replacement backend.
//!
//! This module owns no product behavior. It deserializes one command payload,
//! calls one replacement operation, and publishes that operation's committed
//! mutation receipt.

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{
    Application, FileHash, ItemId, ItemQuery, ItemTarget, Lifecycle, MutationReceipt,
};
use crate::duplicates_v2::ResolutionChoice;
use crate::folders_v2::{
    FolderId, FolderMetadataInput, FolderMutationReceipt, FolderWatchInput,
    ReorderFolderItemsInput, SetFolderAutoTagsInput, SortFolderTreeInput,
};
use crate::navigation_v2::{CreateSmartFolderInput, SmartFolderMutationReceipt};
use crate::operations_v2::MediaMetadataPatch;
use crate::query_v2::ItemPageRequest;
use crate::subscription_catalog_v2::{
    NewSubscription, NewSubscriptionQuery, SubscriptionCoverCandidateCursor,
    SubscriptionCoverSelection, SubscriptionDestinationPolicy,
};
use crate::subscriptions::gallery_dl_runner::normalize_ehentai_gallery_url;

pub fn dispatch(
    application: &Application,
    command: &str,
    args_json: &str,
) -> Result<String, String> {
    match command {
        "items.query" => {
            let input: QueryItemsInput = parse(args_json)?;
            read(crate::query_v2::query_for_application(
                application,
                &input.query,
                input.page,
            )?)
        }
        "items.details" => {
            let input: ItemInput = parse(args_json)?;
            read(crate::query_v2::details(application, input.item_id)?)
        }
        "items.selection_summary" => {
            let input: TargetInput = parse(args_json)?;
            read(crate::query_v2::selection_summary_for_application(
                application,
                &input.target,
            )?)
        }
        "sidebar.counts" => read(crate::query_v2::sidebar_counts_for_application(
            application,
        )?),
        "library.stats" => read(crate::query_v2::library_statistics(application.store())?),
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
        "tags.unused_count" => read(crate::tags_v2::unused_count(application)?),
        "duplicates.list" => {
            let input: LimitInput = parse(args_json)?;
            read(crate::duplicates_v2::list_candidates(
                application,
                input.limit,
            )?)
        }
        "subscriptions.list" => read(crate::subscription_catalog_v2::list(application)?),
        "subscriptions.cover.candidates" => {
            let input: SubscriptionCoverCandidatesInput = parse(args_json)?;
            read(
                crate::subscription_catalog_v2::subscription_cover_candidates(
                    application,
                    input.subscription_id,
                    input.cursor.as_ref(),
                    input.limit,
                )?,
            )
        }
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
        "history.state" => read(application.history_state()?),
        "history.undo" => {
            let output = application.undo()?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "history.redo" => {
            let output = application.redo()?;
            publish_nested(application, output, |output| &output.receipt)
        }
        "settings.view.get" => {
            let input: ScopeInput = parse(args_json)?;
            read(crate::settings_v2::view_preferences(
                application,
                &input.scope,
            )?)
        }
        "tasks.get" => read(crate::tasks_v2::snapshot(application)?),
        "cloud.providers.detect" => read(crate::cloud::provider::detect_roots()),
        "cloud.status.get" => read(crate::cloud::status(application)?),
        "cloud.configuration.get" => read(crate::cloud::configuration(application)?),
        "cloud.configure" => {
            let input: crate::cloud::ConfigureCloudInput = parse(args_json)?;
            publish(application, crate::cloud::configure(application, &input)?)
        }
        "cloud.pause" => {
            let input: CloudPauseInput = parse(args_json)?;
            publish(
                application,
                crate::cloud::set_paused(application, input.paused)?,
            )
        }
        "cloud.retention.update" => {
            let input: ValueInput = parse(args_json)?;
            publish(
                application,
                crate::cloud::update_retention(application, &input.value)?,
            )
        }
        "media.resolve_paths" => {
            let input: FileHashesInput = parse(args_json)?;
            read(crate::media_io_v2::resolve_file_paths(
                application.store(),
                application.blobs(),
                &input.file_hashes,
            )?)
        }
        "media.resolve_target_paths" => {
            let input: TargetInput = parse(args_json)?;
            read(crate::media_io_v2::resolve_target_file_paths(
                application.store(),
                application.projections(),
                application.blobs(),
                &input.target,
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
                application.projections(),
                application.blobs(),
                application.store().library_root(),
                &input,
            )?)
        }
        "items.record_view" => {
            let input: ItemInput = parse(args_json)?;
            publish(application, application.record_recent_view(input.item_id)?)
        }
        "items.clear_recent_views" => publish(application, application.clear_recent_views()?),

        "items.rename" => {
            let input: ItemNameInput = parse(args_json)?;
            publish(
                application,
                application.rename_item(input.item_id, &input.name)?,
            )
        }
        "items.rename_many" => {
            let input: RenameItemsInput = parse(args_json)?;
            publish(application, application.rename_items(&input.renames)?)
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
        "items.organize_into_collection" => {
            let output = application.organize_into_collection(parse(args_json)?)?;
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
        "items.apply_tags" => {
            let input: ApplyTagsInput = parse(args_json)?;
            publish(
                application,
                application.apply_tags(&input.target, &input.tags, input.add)?,
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
        "folders.duplicate" => {
            let input: FolderInput = parse(args_json)?;
            let (folder_id, receipt) = application.duplicate_folder(input.folder_id)?;
            publish_folder(application, CreatedFolder { folder_id, receipt })
        }
        "folders.metadata.set" => publish_folder(
            application,
            application.set_folder_metadata(&parse::<FolderMetadataInput>(args_json)?)?,
        ),
        "folders.auto_tags.get" => {
            let input: FolderInput = parse(args_json)?;
            read(application.folder_auto_tags(input.folder_id)?)
        }
        "folders.auto_tags.set" => publish_folder(
            application,
            application.set_folder_auto_tags(&parse::<SetFolderAutoTagsInput>(args_json)?)?,
        ),
        "folders.cover.get" => {
            let input: FolderInput = parse(args_json)?;
            read(application.folder_cover(input.folder_id)?)
        }
        "folders.cover.set" => publish_folder(
            application,
            application
                .set_folder_cover(&parse::<crate::folders_v2::SetFolderCoverInput>(args_json)?)?,
        ),
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
        "folders.sort_tree" => publish_folder(
            application,
            application.sort_folder_tree(&parse::<SortFolderTreeInput>(args_json)?)?,
        ),
        "folders.items.reorder" => publish_folder(
            application,
            application.reorder_folder_items(&parse::<ReorderFolderItemsInput>(args_json)?)?,
        ),
        "folders.items.sort_name" => {
            let input: FolderInput = parse(args_json)?;
            publish_folder(
                application,
                application.sort_folder_items_by_name(input.folder_id)?,
            )
        }
        "folders.delete" => {
            let input: FolderIdsInput = parse(args_json)?;
            publish_folder(application, application.delete_folders(&input.folder_ids)?)
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
        "tags.delete_unused" => publish(application, application.delete_unused_tags()?),
        "tags.group.rename" => {
            let input: RenameTagGroupInput = parse(args_json)?;
            publish(
                application,
                application.rename_tag_group(&input.namespace, &input.new_namespace)?,
            )
        }
        "tags.group.delete" => {
            let input: TagGroupInput = parse(args_json)?;
            publish(application, application.delete_tag_group(&input.namespace)?)
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
            match crate::duplicates_v2::resolve_automatically(
                application,
                input.file_id_a,
                input.file_id_b,
            )? {
                Some(output) => publish_nested(application, output, |output| &output.receipt),
                None => read(Option::<crate::duplicates_v2::ResolutionResult>::None),
            }
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
        "subscriptions.queries.grouping" => {
            let input: SetSubscriptionQueryGroupingInput = parse(args_json)?;
            publish(
                application,
                application.set_subscription_query_grouping(input.query_id, input.group_posts)?,
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
        "subscriptions.posts_per_run" => {
            let input: SubscriptionPostsPerRunInput = parse(args_json)?;
            publish(
                application,
                application
                    .set_subscription_posts_per_run(input.subscription_id, input.posts_per_run)?,
            )
        }
        "subscriptions.destination" => {
            let input: SubscriptionDestinationInput = parse(args_json)?;
            publish(
                application,
                application
                    .set_subscription_destination(input.subscription_id, &input.destination)?,
            )
        }
        "subscriptions.cover.set" => {
            let input: SubscriptionCoverInput = parse(args_json)?;
            publish(
                application,
                application.set_subscription_cover(input.subscription_id, &input.cover)?,
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
        "settings.view.reset" => publish(application, application.reset_view_preferences()?),
        _ => Err(format!("Unknown replacement command: {command}")),
    }
}

pub async fn dispatch_async(
    application: &Application,
    command: &str,
    args_json: &str,
) -> Result<String, String> {
    if command == "diagnostics.snapshot" {
        return read(crate::diagnostics_v2::snapshot(application)?);
    }
    match command {
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
                return Err("The selected gallery service does not match the URL".to_string());
            }
            // A previous transient run may have disappeared while its source
            // rows were still provisional. Remove that abandoned staging
            // before retrying so the new complete gallery owns one clean set
            // of source items and ingest jobs.
            crate::ingest_queue_v2::discard_abandoned_gallery_sources(application)?;
            let gallery_id = url
                .split('/')
                .nth(4)
                .ok_or_else(|| "E-Hentai gallery URL has no gallery ID".to_string())?;
            let definition = NewSubscription {
                name: format!(
                    "{} Gallery {gallery_id}",
                    if is_exhentai { "ExHentai" } else { "E-Hentai" }
                ),
                schedule: "manual".to_string(),
                initial_post_limit: None,
                periodic_post_limit: None,
                queries: vec![NewSubscriptionQuery {
                    site_id: "ehentai".to_string(),
                    query_text: url,
                    display_name: Some("Gallery import".to_string()),
                    notes: None,
                    group_posts: true,
                }],
            };
            let timestamp = now();
            let (subscription_id, _) =
                application.create_subscription_definition(&definition, &timestamp)?;
            if let Err(error) =
                crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
                    application.store().library_root(),
                    subscription_id,
                )
                .await
            {
                let _ = application.delete_subscription(subscription_id);
                return Err(error);
            }
            let (run, receipt) =
                match application.request_subscription_run(subscription_id, &timestamp) {
                    Ok(result) => result,
                    Err(error) => {
                        let _ = application.delete_subscription(subscription_id);
                        return Err(error);
                    }
                };
            return publish(
                application,
                CreatedSubscriptionRun {
                    run_id: run.run_id,
                    created: run.created,
                    receipt,
                },
            );
        }
        "subscriptions.gallery.cleanup" => {
            let input: SubscriptionInput = parse(args_json)?;
            let catalog = crate::subscription_catalog_v2::list(application)?;
            let Some(subscription) = catalog
                .subscriptions
                .iter()
                .find(|entry| entry.subscription_id == input.subscription_id)
            else {
                return read(Option::<GalleryImportCleanupResult>::None);
            };
            if subscription.queries.len() != 1 || subscription.queries[0].site_id != "ehentai" {
                return Err(
                    "Only transient E-Hentai gallery jobs can use gallery cleanup".to_string(),
                );
            }
            crate::subscriptions::archive::clear_subscription_archive_entries_at_root(
                application.store().library_root(),
                input.subscription_id,
            )
            .await?;
            let title = application.store().read(|connection| {
                connection
                    .query_row(
                        "SELECT NULLIF(TRIM(sp.title), '')
                         FROM subscription_source_post ssp
                         JOIN source_post sp ON sp.source_post_id = ssp.source_post_id
                         WHERE ssp.subscription_id = ?1 AND sp.root_item_id IS NOT NULL
                         ORDER BY sp.updated_at DESC, sp.source_post_id DESC
                         LIMIT 1",
                        [input.subscription_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map(Option::flatten)
            })?;
            return match application.delete_subscription(input.subscription_id) {
                Ok(receipt) => publish(application, GalleryImportCleanupResult { title, receipt }),
                Err(error) if error.contains("subscription does not exist") => {
                    read(Option::<GalleryImportCleanupResult>::None)
                }
                Err(error) => Err(error),
            };
        }
        "cloud.libraries.discover" => {
            let input: CloudRootInput = parse(args_json)?;
            return read(crate::cloud::discover_libraries(&input.root_path).await?);
        }
        "cloud.reconcile" => {
            let provider = crate::cloud::directory_provider(application)?;
            let result = crate::cloud::reconcile::reconcile(
                application,
                &provider,
                crate::cloud::reconcile::ReconcileMode::Manual,
            )
            .await?;
            return read(result);
        }
        "cloud.snapshot.create" => {
            let provider = crate::cloud::directory_provider(application)?;
            return read(crate::cloud::snapshot::publish(application.store(), &provider).await?);
        }
        "cloud.restore.list" => {
            let provider = crate::cloud::directory_provider(application)?;
            return read(
                crate::cloud::snapshot::list_remote(application.store(), &provider).await?,
            );
        }
        "subscriptions.reset" => {
            let input: SubscriptionInput = parse(args_json)?;
            return publish(
                application,
                application
                    .reset_subscription(input.subscription_id)
                    .await?,
            );
        }
        "pixiv_oauth_start" => {
            return read(crate::subscriptions::pixiv_oauth::generate_challenge());
        }
        "pixiv_oauth_exchange" => {
            let input: PixivOAuthExchangeInput = parse(args_json)?;
            let refresh_token =
                crate::subscriptions::pixiv_oauth::exchange_code(&input.code, &input.code_verifier)
                    .await?;
            let cookies = input
                .phpsessid
                .filter(|value| !value.trim().is_empty())
                .map(|value| std::collections::HashMap::from([("PHPSESSID".to_string(), value)]));
            let receipt = crate::auth_v2::set_credential(
                application,
                crate::auth_v2::SetCredentialInput {
                    site_id: "pixiv".to_string(),
                    credential_type: "oauth_token".to_string(),
                    display_name: Some("Pixiv".to_string()),
                    username: None,
                    password: None,
                    cookies,
                    headers: None,
                    oauth_token: Some(refresh_token),
                },
                &now(),
            )?;
            application.publish(&receipt);
            return read(PixivOAuthExchangeOutput { ok: true });
        }
        "ai.status" => return read(crate::ai_runtime_v2::model_status(application).await?),
        "ai.models.download" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models_v2::download(application, &input.slug).await?;
            return read(crate::ai_runtime_v2::model_status(application).await?);
        }
        "ai.models.cancel" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models_v2::cancel_download(application, &input.slug).await?;
            return read(EmptyOutput {});
        }
        "ai.models.delete" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models_v2::delete(application, &input.slug).await?;
            return read(crate::ai_runtime_v2::model_status(application).await?);
        }
        "ai.models.optimize" => {
            let input: ModelInput = parse(args_json)?;
            crate::ai_models_v2::optimize(application, &input.slug).await?;
            return read(crate::ai_runtime_v2::model_status(application).await?);
        }
        "ai.review.predict" => {
            let input: crate::ai_runtime_v2::ManualPredictionRequest = parse(args_json)?;
            return read(crate::ai_runtime_v2::manual_predict(application, input).await?);
        }
        "ai.review.unload" => {
            crate::ai_runtime_v2::unload_sessions(application).await;
            return read(EmptyOutput {});
        }
        "ai.review.apply" => {
            let input: AiAssignmentsInput = parse(args_json)?;
            let assignments = input
                .assignments
                .into_iter()
                .map(|assignment| (assignment.media_item_id, assignment.tags))
                .collect::<Vec<_>>();
            return publish(
                application,
                application.apply_media_tag_assignments(&assignments)?,
            );
        }
        _ => {}
    }
    if command == "media.request_thumbnail" {
        let input: FileHashInput = parse(args_json)?;
        return read(crate::media_io_v2::request_thumbnail(
            application.store(),
            application.blobs(),
            &input.file_hash,
        )?);
    }
    if command == "media.render_thumbnail_now" {
        let input: FileHashInput = parse(args_json)?;
        return read(
            crate::media_io_v2::render_thumbnail_now(
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

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudRootInput {
    root_path: String,
}

#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CloudRestorePrepared {
    pub snapshot_id: String,
    pub restored: bool,
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

#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreatedFolder {
    folder_id: FolderId,
    receipt: FolderMutationReceipt,
}
impl FolderReceipt for CreatedFolder {
    fn folder_receipt(&self) -> &FolderMutationReceipt {
        &self.receipt
    }
}

#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreatedSmartFolder {
    #[ts(type = "number")]
    smart_folder_id: i64,
    receipt: SmartFolderMutationReceipt,
}
impl SmartReceipt for CreatedSmartFolder {
    fn smart_receipt(&self) -> &SmartFolderMutationReceipt {
        &self.receipt
    }
}

#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreatedSubscription {
    #[ts(type = "number")]
    subscription_id: i64,
    receipt: MutationReceipt,
}
#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreatedSubscriptionQuery {
    #[ts(type = "number")]
    query_id: i64,
    receipt: MutationReceipt,
}
#[derive(Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct CreatedSubscriptionRun {
    #[ts(type = "number")]
    run_id: i64,
    created: bool,
    receipt: MutationReceipt,
}
#[derive(Serialize)]
pub struct GalleryImportCleanupResult {
    title: Option<String>,
    receipt: MutationReceipt,
}
receipt_field!(CreatedSubscription);
receipt_field!(CreatedSubscriptionQuery);
receipt_field!(CreatedSubscriptionRun);
receipt_field!(GalleryImportCleanupResult);

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct QueryItemsInput {
    query: ItemQuery,
    page: ItemPageRequest,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemInput {
    item_id: ItemId,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemNameInput {
    item_id: ItemId,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RenameItemsInput {
    pub renames: Vec<crate::operations_v2::ItemRename>,
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
pub struct TargetInput {
    target: ItemTarget,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LifecycleInput {
    target: ItemTarget,
    lifecycle: Lifecycle,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderMembershipInput {
    target: ItemTarget,
    #[ts(type = "number")]
    folder_id: i64,
    present: bool,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ApplyTagsInput {
    target: ItemTarget,
    tags: Vec<String>,
    add: bool,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct PatchMetadataInput {
    target: ItemTarget,
    patch: MediaMetadataPatch,
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
pub struct TagInput {
    #[ts(type = "number")]
    tag_id: i64,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RenameTagInput {
    #[ts(type = "number")]
    tag_id: i64,
    name: String,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RenameTagGroupInput {
    namespace: String,
    new_namespace: String,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct TagGroupInput {
    namespace: String,
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
pub struct ResolveDuplicateInput {
    #[ts(type = "number")]
    file_id_a: i64,
    #[ts(type = "number")]
    file_id_b: i64,
    choice: ResolutionChoice,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AutomaticDuplicateInput {
    #[ts(type = "number")]
    file_id_a: i64,
    #[ts(type = "number")]
    file_id_b: i64,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderInput {
    folder_id: FolderId,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderIdsInput {
    folder_ids: Vec<FolderId>,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RenameFolderInput {
    folder_id: FolderId,
    name: String,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MoveFolderInput {
    folder_id: FolderId,
    parent_id: Option<FolderId>,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SmartFolderInput {
    #[ts(type = "number")]
    smart_folder_id: i64,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct UpdateSmartFolderInput {
    #[ts(type = "number")]
    smart_folder_id: i64,
    value: CreateSmartFolderInput,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MoveSmartFolderInput {
    #[ts(type = "number")]
    smart_folder_id: i64,
    #[ts(type = "number | null")]
    parent_id: Option<i64>,
}
#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ReorderSmartFoldersInput {
    #[ts(type = "number | null")]
    parent_id: Option<i64>,
    #[ts(type = "number[]")]
    smart_folder_ids: Vec<i64>,
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

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AiTagAssignment {
    media_item_id: ItemId,
    tags: Vec<String>,
}

#[derive(Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct AiAssignmentsInput {
    assignments: Vec<AiTagAssignment>,
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
    crate::duplicates_v2::DEFAULT_GLOBAL_DISTANCE_THRESHOLD
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
                crate::canonical_bitmap::seed_test_state(transaction, &Default::default())?;
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
        assert_eq!(page(&application, "all").visible_item_count, Some(1));
        assert_eq!(page(&application, "inbox").visible_item_count, Some(0));

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
        assert_eq!(page(&application, "all").visible_item_count, Some(0));
        assert_eq!(page(&application, "inbox").visible_item_count, Some(1));

        dispatch(&application, "history.undo", "{}").unwrap();
        assert_eq!(page(&application, "all").visible_item_count, Some(1));
        assert_eq!(page(&application, "inbox").visible_item_count, Some(0));
    }

    #[test]
    fn range_target_round_trips_through_the_typed_ipc_boundary() {
        let (_directory, application, item_id) = fixture();
        let output = dispatch(
            &application,
            "items.selection_summary",
            &format!(
                r#"{{"target":{{"kind":"range","query":{{"scope":{{"kind":"all"}}}},"anchor_item_id":{},"focus_item_id":{}}}}}"#,
                item_id.0, item_id.0
            ),
        )
        .unwrap();
        let summary: crate::query_v2::SelectionSummary = serde_json::from_str(&output).unwrap();
        assert_eq!(summary.selected_count, 1);
        assert_eq!(
            summary.sample_hashes,
            vec![FileHash("ipc-hash".to_string())]
        );
    }

    #[test]
    fn tag_history_rebuilds_projection_state() {
        let (_directory, application, item_id) = fixture();
        assert_eq!(page(&application, "untagged").visible_item_count, Some(1));
        dispatch(
            &application,
            "items.apply_tags",
            &format!(
                r#"{{"target":{{"kind":"explicit","item_ids":[{}]}},"tags":["general:test"],"add":true}}"#,
                item_id.0
            ),
        )
        .unwrap();
        assert_eq!(page(&application, "untagged").visible_item_count, Some(0));

        dispatch(&application, "history.undo", "{}").unwrap();
        assert_eq!(page(&application, "untagged").visible_item_count, Some(1));
        dispatch(&application, "history.redo", "{}").unwrap();
        assert_eq!(page(&application, "untagged").visible_item_count, Some(0));
    }

    #[test]
    fn folder_auto_tags_get_returns_the_tag_array_without_a_result_wrapper() {
        let (_directory, application, _) = fixture();
        let created: serde_json::Value = serde_json::from_str(
            &dispatch(
                &application,
                "folders.create",
                r#"{"name":"References","parent_id":null}"#,
            )
            .unwrap(),
        )
        .unwrap();

        dispatch(
            &application,
            "folders.auto_tags.set",
            &format!(
                r#"{{"folder_id":{},"tags":["creator:alice","rating:safe"]}}"#,
                created["folder_id"].as_i64().unwrap()
            ),
        )
        .unwrap();

        let output = dispatch(
            &application,
            "folders.auto_tags.get",
            &format!(
                r#"{{"folder_id":{}}}"#,
                created["folder_id"].as_i64().unwrap()
            ),
        )
        .unwrap();
        let tags: Vec<String> = serde_json::from_str(&output).unwrap();
        assert_eq!(tags, vec!["creator:alice", "rating:safe"]);
    }

    #[tokio::test]
    async fn gallery_cleanup_returns_the_downloaded_title_without_creating_history() {
        let (_directory, application, item_id) = fixture();
        let definition = NewSubscription {
            name: "E-Hentai Gallery 12345".to_string(),
            schedule: "manual".to_string(),
            initial_post_limit: None,
            periodic_post_limit: None,
            queries: vec![NewSubscriptionQuery {
                site_id: "ehentai".to_string(),
                query_text: "https://e-hentai.org/g/12345/0123456789/".to_string(),
                display_name: Some("Gallery import".to_string()),
                notes: None,
                group_posts: true,
            }],
        };
        let (subscription_id, _) = application
            .create_subscription_definition(&definition, "2026-08-26T00:00:00Z")
            .unwrap();
        application
            .store()
            .transaction(|transaction| {
                let query_id: i64 = transaction.query_row(
                    "SELECT query_id FROM subscription_query WHERE subscription_id = ?1",
                    [subscription_id],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO source_post
                     (site_id, post_key, title, root_item_id, created_at, updated_at)
                 VALUES ('ehentai', '12345', 'Example Gallery', ?1, 'now', 'now')",
                    [item_id.0],
                )?;
                let source_post_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO subscription_source_post
                     (subscription_id, query_id, source_post_id)
                 VALUES (?1, ?2, ?3)",
                    params![subscription_id, query_id, source_post_id],
                )?;
                Ok(())
            })
            .unwrap();

        let output: serde_json::Value = serde_json::from_str(
            &dispatch_async(
                &application,
                "subscriptions.gallery.cleanup",
                &format!(r#"{{"subscription_id":{subscription_id}}}"#),
            )
            .await
            .unwrap(),
        )
        .unwrap();

        assert_eq!(output["title"], "Example Gallery");
        assert!(application.history_state().unwrap().undo.is_none());
    }

    #[test]
    fn rename_history_round_trips_through_ipc() {
        let (_directory, application, item_id) = fixture();
        dispatch(
            &application,
            "items.rename",
            &format!(r#"{{"item_id":{},"name":"After"}}"#, item_id.0),
        )
        .unwrap();

        let state: serde_json::Value =
            serde_json::from_str(&dispatch(&application, "history.state", "{}").unwrap()).unwrap();
        assert_eq!(state["undo"]["label"], "Rename item");
        assert!(state["redo"].is_null());

        let undo: serde_json::Value =
            serde_json::from_str(&dispatch(&application, "history.undo", "{}").unwrap()).unwrap();
        assert_eq!(undo["entry"]["label"], "Rename item");
        let name: Option<String> = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT (SELECT name FROM root_metadata WHERE root_item_id = ?1)",
                    [item_id.0],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(name, None);

        dispatch(&application, "history.redo", "{}").unwrap();
        let name: String = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT (SELECT name FROM root_metadata WHERE root_item_id = ?1)",
                    [item_id.0],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(name, "After");
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

    #[tokio::test]
    async fn ai_commands_share_the_replacement_item_and_invalidation_contract() {
        let (_directory, application, item_id) = fixture();

        let status = dispatch_async(&application, "ai.status", "{}")
            .await
            .unwrap();
        let status: crate::ai_runtime_v2::AiRuntimeStatus = serde_json::from_str(&status).unwrap();
        assert_eq!(
            status.models.len(),
            crate::ai_tagger::models::known_models().len()
        );

        let output = dispatch_async(
            &application,
            "ai.review.apply",
            &format!(
                r#"{{"assignments":[{{"media_item_id":{},"tags":["general:one girl"]}}]}}"#,
                item_id.0
            ),
        )
        .await
        .unwrap();
        let receipt: MutationReceipt = serde_json::from_str(&output).unwrap();
        assert_eq!(receipt.item_ids, vec![item_id]);
        assert!(receipt.resources.iter().any(|resource| resource == "tags"));
        let tag_id: i64 = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT tag_id FROM tag
                     WHERE namespace = 'general' AND subtag = 'one girl'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(application
            .projections()
            .direct_tag_bitmap(tag_id)
            .contains(item_id.0 as u32));
    }
}
