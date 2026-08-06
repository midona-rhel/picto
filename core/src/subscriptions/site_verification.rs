//! Per-site end-to-end verification.
//!
//! Walks the exact production download path for one site — URL building,
//! credential resolution, gallery-dl bridge run, metadata adaptation,
//! schema validation — without creating a subscription, touching the
//! ingest queue, or writing credential health/issues. Downloads go to the
//! runner's temp dir and are deleted afterwards.

use std::collections::HashMap;

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::state::AppState;
use crate::subscriptions::credential_service::SubscriptionCredentialService;
use crate::subscriptions::gallery_dl_runner::{
    self, classify_failure, cleanup_temp_dir, get_site_metadata_schema, validate_site_metadata,
    FailureKind, GalleryDlRunner, RunOptions, SiteMetadataValidationResult,
};
use crate::subscriptions::source_adapter::DownloadedItem;

const MAX_VERIFY_POSTS: u32 = 3;
const STDERR_TAIL_LINES: usize = 10;

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SiteVerificationItemReport {
    pub post_id: Option<String>,
    pub tag_count: usize,
    /// Tag counts keyed by namespace ("" = general).
    pub namespaced_tag_counts: HashMap<String, usize>,
    pub page_num: Option<u32>,
    pub page_count: Option<u32>,
    pub canonical_post_url: Option<String>,
    pub created_at_present: bool,
    pub creator_present: bool,
    /// Present only for sites with a metadata schema (pixiv/gelbooru/danbooru).
    #[ts(type = "unknown | null")]
    pub schema_validation: Option<SiteMetadataValidationResult>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SiteVerificationReport {
    pub site_id: String,
    pub url: String,
    /// "used:<lookup_key>" | "missing_optional" | "missing_required" | "unsupported"
    pub credential_state: String,
    pub exit_code: Option<i32>,
    pub failure_kind: Option<String>,
    pub stderr_tail: String,
    pub discovered: usize,
    pub downloaded: usize,
    pub skipped_archive: usize,
    pub items: Vec<SiteVerificationItemReport>,
    pub passed: bool,
    pub failure_reasons: Vec<String>,
}

fn item_report(site_id: &str, url: &str, item: &DownloadedItem) -> SiteVerificationItemReport {
    let meta = &item.metadata;
    let mut namespaced: HashMap<String, usize> = HashMap::new();
    let mut creator_present = false;
    for (ns, _) in &meta.tags {
        *namespaced.entry(ns.clone()).or_insert(0) += 1;
        if ns == "creator" {
            creator_present = true;
        }
    }
    let schema_validation = get_site_metadata_schema(site_id)
        .map(|_| validate_site_metadata(site_id, url, meta.raw_metadata.as_ref()));
    SiteVerificationItemReport {
        post_id: meta.post_id.clone(),
        tag_count: meta.tags.len(),
        namespaced_tag_counts: namespaced,
        page_num: meta.page_num,
        page_count: meta.page_count,
        canonical_post_url: meta.canonical_post_url.clone(),
        created_at_present: meta.created_at.is_some(),
        creator_present,
        schema_validation,
    }
}

fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines[start..].join("\n")
}

/// Run a small end-to-end probe for one site. Read-only with respect to
/// library state: nothing is ingested and no issues/health rows are written.
pub async fn verify_site(
    state: &AppState,
    site_id: &str,
    query: Option<&str>,
    post_limit: Option<u32>,
) -> Result<SiteVerificationReport, String> {
    let site =
        gallery_dl_runner::site_by_id(site_id).ok_or_else(|| format!("Unknown site: {site_id}"))?;
    // Account-style example queries are placeholders ("12345", "username") —
    // a probe against them can prove the extractor runs but not that content
    // flows. Callers should pass a real account query for a conclusive probe.
    let used_placeholder_query = query.is_none() && !site.supports_query;
    let query = query.unwrap_or(site.example_query);
    let url = gallery_dl_runner::build_url(site_id, query)
        .ok_or_else(|| format!("Could not build URL for site: {site_id}"))?;

    // Side-effect-free credential lookup — verification must not write
    // health rows or subscription issues.
    let credential_service = SubscriptionCredentialService::new(state.engine.db());
    let resolved = credential_service.resolve_credential(site_id, &url);
    let credential_missing = resolved.auth_supported && resolved.gallery_dl_auth.is_none();
    let credential_state = match (&resolved.matched_lookup_key, resolved.auth_supported) {
        (Some(key), _) => format!("used:{key}"),
        (None, true) if resolved.auth_required_for_full_access => "missing_required".to_string(),
        (None, true) => "missing_optional".to_string(),
        (None, false) => "unsupported".to_string(),
    };

    if credential_missing && resolved.auth_required_for_full_access {
        // Don't burn a live request that will predictably fail or return a
        // limited result set — report as skipped, distinct from breakage.
        return Ok(SiteVerificationReport {
            site_id: site_id.to_string(),
            url,
            credential_state,
            exit_code: None,
            failure_kind: None,
            stderr_tail: String::new(),
            discovered: 0,
            downloaded: 0,
            skipped_archive: 0,
            items: Vec::new(),
            passed: false,
            failure_reasons: vec!["skipped: credential missing".to_string()],
        });
    }

    let binary_path = crate::media_processing::gallery_dl_path::gallery_dl_path()?.clone();
    let runner = GalleryDlRunner::new(binary_path);
    let post_limit = post_limit
        .unwrap_or(MAX_VERIFY_POSTS)
        .clamp(1, MAX_VERIFY_POSTS);

    let opts = RunOptions {
        subscription_id: None,
        query_id: None,
        site_id: site_id.to_string(),
        url: url.clone(),
        post_limit: Some(post_limit),
        range_start: 1,
        abort_threshold: None,
        auth: resolved.gallery_dl_auth,
        // Empty path → runner skips the download archive entirely.
        archive_path: std::path::PathBuf::new(),
        archive_prefix: None,
        cancel: CancellationToken::new(),
    };

    let (item_tx, mut item_rx) = tokio::sync::mpsc::channel::<DownloadedItem>(16);
    let run_future = runner.run(&opts, item_tx);
    tokio::pin!(run_future);

    let mut items: Vec<SiteVerificationItemReport> = Vec::new();
    let mut downloaded = 0usize;
    let summary = loop {
        tokio::select! {
            item = item_rx.recv() => {
                if let Some(item) = item {
                    downloaded += 1;
                    items.push(item_report(site_id, &url, &item));
                }
            }
            result = &mut run_future => {
                // Drain any items still buffered in the channel.
                while let Ok(item) = item_rx.try_recv() {
                    downloaded += 1;
                    items.push(item_report(site_id, &url, &item));
                }
                break result;
            }
        }
    };

    let summary = match summary {
        Ok(summary) => summary,
        Err(error) => {
            return Ok(SiteVerificationReport {
                site_id: site_id.to_string(),
                url,
                credential_state,
                exit_code: None,
                failure_kind: Some(FailureKind::Environment.as_str().to_string()),
                stderr_tail: error.clone(),
                discovered: 0,
                downloaded,
                skipped_archive: 0,
                items,
                passed: false,
                failure_reasons: vec![format!("runner error: {error}")],
            });
        }
    };

    cleanup_temp_dir(&summary.temp_dir).await;

    let mut failure_reasons = Vec::new();
    let classified = classify_failure(&summary.stderr_output);
    let placeholder_miss = used_placeholder_query
        && downloaded == 0
        && (summary.exit_code == 0 || classified == FailureKind::NotFound);
    if placeholder_miss {
        // The extractor ran and the site answered — the fake example account
        // just has nothing. Inconclusive, not broken.
        failure_reasons.push(
            "inconclusive: placeholder account query — pass --query with a real account"
                .to_string(),
        );
    }
    if summary.exit_code != 0 && !placeholder_miss {
        failure_reasons.push(format!(
            "gallery-dl exited with {} ({:?})",
            summary.exit_code, classified
        ));
    }
    if summary.discovered_items == 0 && summary.skipped_archive_items == 0 && !placeholder_miss {
        failure_reasons.push("no items discovered".to_string());
    }
    let items_missing_post_id = items.iter().filter(|i| i.post_id.is_none()).count();
    if downloaded > 0 && items_missing_post_id > 0 {
        failure_reasons.push(format!(
            "{items_missing_post_id}/{downloaded} items missing post_id"
        ));
    }
    // Query-style booru sites must produce tags; account-style sites may be
    // tagless by design (coomer, webtoons, instagram...).
    if site.supports_query && downloaded > 0 {
        let tagless = items.iter().filter(|i| i.tag_count == 0).count();
        if tagless == downloaded {
            failure_reasons.push(format!("0/{downloaded} items had tags"));
        }
    }
    for item in &items {
        if let Some(validation) = &item.schema_validation {
            if !validation.valid {
                failure_reasons.push(format!(
                    "schema validation failed: missing {:?}, invalid {:?}",
                    validation.missing_required_fields, validation.invalid_fields
                ));
                break;
            }
        }
    }

    let failure_kind = (summary.exit_code != 0).then(|| {
        classify_failure(&summary.stderr_output)
            .as_str()
            .to_string()
    });

    Ok(SiteVerificationReport {
        site_id: site_id.to_string(),
        url,
        credential_state,
        exit_code: Some(summary.exit_code),
        failure_kind,
        stderr_tail: stderr_tail(&summary.stderr_output),
        discovered: summary.discovered_items,
        downloaded,
        skipped_archive: summary.skipped_archive_items,
        items,
        passed: failure_reasons.is_empty(),
        failure_reasons,
    })
}
