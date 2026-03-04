//! Integration tests for gallery-dl downloading.
//!
//! These tests actually invoke gallery-dl to download a known post and verify
//! that the file + metadata sidecar are correct. Modelled after hydownloader's
//! test suite.
//!
//! Run with: `cargo test -p picto_core --test gallery_dl_integration -- --ignored`
//!
//! Prerequisites:
//! - gallery-dl installed (via vendor/ or system PATH)
//! - Network access

use picto_core::gallery_dl_runner::{
    self, build_url, site_by_id, GalleryDlRunner, RunOptions, SITES,
};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

fn find_gallery_dl() -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    #[cfg(target_os = "windows")]
    let sep = ';';
    #[cfg(not(target_os = "windows"))]
    let sep = ':';
    for dir in path_var.split(sep) {
        let candidate = PathBuf::from(dir).join("gallery-dl");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Download a single known Danbooru post and verify file + namespaced tags.
#[tokio::test]
#[ignore]
async fn test_download_danbooru_post() {
    let binary = match find_gallery_dl() {
        Some(p) => p,
        None => {
            eprintln!("gallery-dl not found in PATH, skipping");
            return;
        }
    };

    let runner = GalleryDlRunner::new(binary);
    let archive_path = std::env::temp_dir().join("picto-test-archive-dan.sqlite3");
    // Clean up stale archive so the post isn't skipped from a previous run.
    let _ = std::fs::remove_file(&archive_path);

    let opts = RunOptions {
        url: "https://danbooru.donmai.us/posts/10873290".into(),
        file_limit: Some(1),
        abort_threshold: None,
        sleep_request: 1.0,
        credential: None,
        archive_path,
        archive_prefix: None,
        cancel: CancellationToken::new(),
    };

    let result = runner.run(&opts).await.expect("gallery-dl run failed");

    assert!(
        result.exit_code == 0,
        "Exit code: {} (stderr: {})",
        result.exit_code,
        result.stderr_output,
    );

    assert!(!result.items.is_empty(), "No items downloaded");

    let item = &result.items[0];
    assert!(item.file_path.exists());

    let file_size = std::fs::metadata(&item.file_path).expect("stat").len();
    assert!(file_size > 0, "Downloaded file is empty");

    assert!(
        !item.metadata.tags.is_empty(),
        "Danbooru post should have tags"
    );

    // Print metadata for inspection
    eprintln!("--- Danbooru post metadata ---");
    eprintln!("  Tags ({}):", item.metadata.tags.len());
    for (ns, tag) in &item.metadata.tags {
        if ns.is_empty() {
            eprintln!("    {tag}");
        } else {
            eprintln!("    {ns}:{tag}");
        }
    }
    eprintln!("  Description: {:?}", item.metadata.description);
    eprintln!("  Source URL: {:?}", item.metadata.source_url);
    eprintln!("  Title: {:?}", item.metadata.title);
    eprintln!("  Post ID: {:?}", item.metadata.post_id);
    eprintln!("  Category: {:?}", item.metadata.category);

    // Danbooru tags should have namespaces (creator, character, etc.)
    let has_namespaced = item.metadata.tags.iter().any(|(ns, _)| !ns.is_empty());
    assert!(
        has_namespaced,
        "Danbooru tags should include namespaced tags (creator, character, etc.)"
    );
}

/// Download a single Yande.re post (no auth required).
#[tokio::test]
#[ignore]
async fn test_download_yandere_post() {
    let binary = match find_gallery_dl() {
        Some(p) => p,
        None => {
            eprintln!("gallery-dl not found in PATH, skipping");
            return;
        }
    };

    let runner = GalleryDlRunner::new(binary);
    let archive_path = std::env::temp_dir().join("picto-test-archive-yan.sqlite3");
    let _ = std::fs::remove_file(&archive_path);

    let opts = RunOptions {
        url: "https://yande.re/post/show/1200000".into(),
        file_limit: Some(1),
        abort_threshold: None,
        sleep_request: 1.0,
        credential: None,
        archive_path,
        archive_prefix: None,
        cancel: CancellationToken::new(),
    };

    let result = runner.run(&opts).await.expect("gallery-dl run failed");

    assert!(
        result.exit_code == 0,
        "Exit code: {} (stderr: {})",
        result.exit_code,
        result.stderr_output,
    );

    assert!(!result.items.is_empty(), "No items downloaded");

    let item = &result.items[0];
    assert!(item.file_path.exists());
    assert!(std::fs::metadata(&item.file_path).unwrap().len() > 0);
    assert!(
        !item.metadata.tags.is_empty(),
        "Yandere post should have tags"
    );
}

/// Use the site registry to build a tag search URL and download from Yande.re.
#[tokio::test]
#[ignore]
async fn test_tag_search_yandere() {
    let binary = match find_gallery_dl() {
        Some(p) => p,
        None => {
            eprintln!("gallery-dl not found in PATH, skipping");
            return;
        }
    };

    let url = build_url("yandere", "landscape").expect("yandere should be in site registry");
    assert!(url.contains("yande.re"));
    assert!(url.contains("landscape"));

    let runner = GalleryDlRunner::new(binary);
    let archive_path = std::env::temp_dir().join("picto-test-archive-search.sqlite3");
    let _ = std::fs::remove_file(&archive_path);

    let opts = RunOptions {
        url,
        file_limit: Some(2),
        abort_threshold: None,
        sleep_request: 1.0,
        credential: None,
        archive_path,
        archive_prefix: None,
        cancel: CancellationToken::new(),
    };

    let result = runner.run(&opts).await.expect("gallery-dl run failed");

    assert!(
        result.exit_code == 0,
        "Exit code: {} (stderr: {})",
        result.exit_code,
        result.stderr_output,
    );

    assert!(
        !result.items.is_empty(),
        "Tag search should have found files (stderr: {})",
        result.stderr_output,
    );

    for item in &result.items {
        assert!(
            item.file_path.exists(),
            "File missing: {}",
            item.file_path.display()
        );
        assert!(
            std::fs::metadata(&item.file_path).unwrap().len() > 0,
            "File is empty: {}",
            item.file_path.display(),
        );
    }
}

/// Verify that the site registry covers all expected sites and URLs are valid.
#[test]
fn test_site_registry_coverage() {
    let required = [
        "danbooru",
        "e621",
        "gelbooru",
        "yandere",
        "rule34",
        "pixiv",
        "safebooru",
    ];
    for id in required {
        assert!(
            site_by_id(id).is_some(),
            "Required site '{}' missing from registry",
            id,
        );
    }

    for site in SITES {
        let url = gallery_dl_runner::substitute_query(site.url_template, "test_query");
        assert!(
            url.starts_with("https://"),
            "Site '{}' URL should start with https://: {}",
            site.id,
            url,
        );
    }
}
