use std::path::Path;

use super::{GalleryDlAuthConfig, RunOptions};

pub fn build_config(opts: &RunOptions, _temp_dir: &Path) -> serde_json::Value {
    let mut extractor = serde_json::Map::new();

    // Request pacing is NOT configured here. The bridge enforces the
    // one-request-per-second/domain policy with a reservation limiter at the
    // requests.Session boundary (scripts/gallery_dl_bridge.py), covering
    // extractor requests and media downloads on one shared per-host clock.
    // gallery-dl's own `sleep`/`sleep-request` measure from the previous
    // response instead, so layering them stacks each request's latency onto
    // the interval and slows runs ~30% below the allowed rate.
    extractor.insert("metadata".into(), serde_json::Value::Bool(true));

    // Moebooru and Gelbooru-v0.2 expose categorized tags through the post
    // page. e621 already returns categorized tags from its API response.
    if matches!(
        opts.site_id.as_str(),
        "gelbooru" | "danbooru" | "yandere" | "konachan" | "safebooru" | "idolcomplex" | "sankaku"
    ) {
        extractor.insert(opts.site_id.clone(), serde_json::json!({"tags": true}));
    }
    if matches!(opts.site_id.as_str(), "idolcomplex" | "sankaku") {
        let site = extractor
            .entry(opts.site_id.clone())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(site) = site.as_object_mut() {
            if let Some(limit) = opts.post_limit.filter(|limit| *limit > 0) {
                site.insert("picto-page-size".into(), serde_json::json!(limit));
            }
            if let Some(cursor) = opts
                .source_cursor
                .as_ref()
                .filter(|cursor| !cursor.trim().is_empty())
            {
                site.insert("picto-next".into(), serde_json::json!(cursor));
            }
        }
    }
    if opts.site_id == "patreon" {
        let site = extractor
            .entry("patreon")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(site) = site.as_object_mut() {
            if let Some(limit) = opts.post_limit.filter(|limit| *limit > 0) {
                site.insert("picto-post-limit".into(), serde_json::json!(limit));
            }
            if opts.range_start > 1 {
                site.insert(
                    "picto-post-skip".into(),
                    serde_json::json!(opts.range_start - 1),
                );
            }
            if let Some(cursor) = opts
                .source_cursor
                .as_ref()
                .filter(|cursor| !cursor.is_empty() && cursor.as_str() != "patreon:first-page")
            {
                site.insert("cursor".into(), serde_json::json!(cursor));
            }
        }
    }
    // Keep gallery-dl in charge of DeviantArt access and URL signing. Captured
    // direct-site cookies are merged below; Picto only disables its own
    // quality rewrite so the source-authorized representation is preserved.
    if opts.site_id == "deviantart" {
        let mut deviantart = serde_json::Map::new();
        deviantart.insert("quality".into(), serde_json::Value::Null);
        if let Some(limit) = opts.post_limit.filter(|limit| *limit > 0) {
            deviantart.insert("picto-post-limit".into(), serde_json::json!(limit));
        }
        if opts.range_start > 1 {
            deviantart.insert(
                "picto-post-skip".into(),
                serde_json::json!(opts.range_start - 1),
            );
        }
        extractor.insert("deviantart".into(), serde_json::Value::Object(deviantart));
    }
    if opts.site_id == "twitter" {
        // gallery-dl already defaults retweets off; pin it so an upstream
        // default change can never reintroduce reposted content.
        extractor.insert("twitter".into(), serde_json::json!({"retweets": false}));
    }
    if opts.site_id == "tumblr" {
        let mut tumblr = serde_json::Map::new();
        // gallery-dl includes reblogs by default; subscriptions follow the
        // blog's own posts only.
        tumblr.insert("reblogs".into(), serde_json::Value::Bool(false));
        tumblr.insert(
            "offset".into(),
            serde_json::json!(opts.range_start.saturating_sub(1)),
        );
        if let Some(limit) = opts.post_limit.filter(|limit| *limit > 0) {
            tumblr.insert("picto-post-limit".into(), serde_json::json!(limit));
        }
        extractor.insert("tumblr".into(), serde_json::Value::Object(tumblr));
    }
    if let Some(prefix) = opts
        .archive_prefix
        .as_ref()
        .filter(|s| !s.trim().is_empty())
    {
        extractor.insert(
            "archive-prefix".into(),
            serde_json::Value::String(prefix.clone()),
        );
    }

    if let Some(ref auth) = opts.auth {
        apply_credential_auth(&mut extractor, auth);
    }

    // Ugoira postprocessor: convert Pixiv animation ZIPs to WebM via ffmpeg.
    let mut postprocessors = Vec::new();
    if let Ok(ffmpeg) = crate::media_processing::ffmpeg_path::ffmpeg_path() {
        let mut ugoira = serde_json::Map::new();
        ugoira.insert("name".into(), serde_json::Value::String("ugoira".into()));
        ugoira.insert(
            "ffmpeg-location".into(),
            serde_json::Value::String(ffmpeg.display().to_string()),
        );
        ugoira.insert("extension".into(), serde_json::Value::String("webm".into()));
        ugoira.insert("keep-files".into(), serde_json::Value::Bool(false));
        ugoira.insert("skip".into(), serde_json::Value::Bool(true));
        postprocessors.push(serde_json::Value::Object(ugoira));
    }

    let mut root = serde_json::Map::new();
    root.insert(
        "base-directory".into(),
        serde_json::Value::String(_temp_dir.display().to_string()),
    );
    root.insert("extractor".into(), serde_json::Value::Object(extractor));

    if !postprocessors.is_empty() {
        extractor = root
            .remove("extractor")
            .and_then(|v| match v {
                serde_json::Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        extractor.insert(
            "postprocessors".into(),
            serde_json::Value::Array(postprocessors),
        );
        root.insert("extractor".into(), serde_json::Value::Object(extractor));
    }

    let mut output = serde_json::Map::new();
    output.insert("progress".into(), serde_json::Value::Bool(false));
    root.insert("output".into(), serde_json::Value::Object(output));

    // Restrict filenames to ASCII-safe characters: [0-9A-Za-z_.] only.
    // Replaces everything else (colons, emoji, CJK, etc.) with underscores.
    // Fixes: macOS NotADirectoryError from `:` in filenames (HFS+ path separator),
    //        Windows OSError [Errno 22] from unsupported Unicode.
    {
        let extractor = root.get_mut("extractor").and_then(|v| v.as_object_mut());
        if let Some(ext) = extractor {
            ext.insert(
                "path-restrict".into(),
                serde_json::Value::String("ascii".into()),
            );
        }
    }

    serde_json::Value::Object(root)
}

fn apply_credential_auth(
    extractor: &mut serde_json::Map<String, serde_json::Value>,
    auth: &GalleryDlAuthConfig,
) {
    // E-Hentai and ExHentai share gallery-dl's `exhentai` extractor.
    let extractor_category = match auth.site_category.as_str() {
        "ehentai" => "exhentai",
        category => category,
    };
    let site_obj = extractor
        .entry(extractor_category.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    match (site_obj, auth.fragment.clone()) {
        (serde_json::Value::Object(site_map), serde_json::Value::Object(auth_map)) => {
            for (key, value) in auth_map {
                site_map.insert(key, value);
            }
        }
        (site_obj, auth) => {
            *site_obj = auth;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_config, GalleryDlAuthConfig, RunOptions};

    #[test]
    fn subscriptions_never_include_reposted_content() {
        for (site, key) in [("tumblr", "reblogs"), ("twitter", "retweets")] {
            let mut opts = RunOptions {
                subscription_id: Some(1),
                query_id: Some(1),
                site_id: site.to_string(),
                url: format!("https://{site}.example/feed"),
                post_limit: Some(10),
                range_start: 1,
                source_cursor: None,
                abort_threshold: None,
                auth: None,
                archive_path: std::path::PathBuf::new(),
                archive_prefix: None,
                pacing_state_dir: std::path::PathBuf::new(),
                cancel: Default::default(),
            };
            opts.site_id = site.to_string();
            let config = build_config(&opts, std::path::Path::new("/tmp"));
            let value = config["extractor"][site][key].as_bool();
            assert_eq!(value, Some(false), "{site} must exclude {key}");
        }
    }

    #[test]
    fn build_config_paces_requests_and_keeps_booru_tags_with_credentials() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "gelbooru".to_string(),
            url: "https://gelbooru.com/index.php?page=post&s=list&tags=test".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: Some(GalleryDlAuthConfig {
                site_category: "gelbooru".to_string(),
                fragment: serde_json::json!({
                    "api-key": "secret",
                    "user-id": "277923",
                }),
            }),
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let extractor = config
            .get("extractor")
            .and_then(|value| value.as_object())
            .expect("extractor config");
        // Pacing lives in the bridge's session limiter; gallery-dl's own
        // response-relative sleeps must stay unset or they stack on it.
        assert!(extractor.get("sleep-request").is_none());
        assert!(extractor.get("sleep").is_none());
        let gelbooru = config
            .get("extractor")
            .and_then(|value| value.get("gelbooru"))
            .and_then(|value| value.as_object())
            .expect("gelbooru extractor config");

        assert_eq!(
            gelbooru.get("tags").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            gelbooru.get("api-key").and_then(|value| value.as_str()),
            Some("secret")
        );
        assert_eq!(
            gelbooru.get("user-id").and_then(|value| value.as_str()),
            Some("277923")
        );
    }

    #[test]
    fn ehentai_credentials_target_gallery_dls_shared_exhentai_extractor() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "ehentai".to_string(),
            url: "https://exhentai.org/g/1169267/1d91fd6979/".to_string(),
            post_limit: None,
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: Some(GalleryDlAuthConfig {
                site_category: "ehentai".to_string(),
                fragment: serde_json::json!({
                    "cookies": { "ipb_member_id": "member" },
                }),
            }),
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let extractor = config
            .get("extractor")
            .and_then(|value| value.as_object())
            .expect("extractor config");
        assert!(extractor.get("ehentai").is_none());
        assert_eq!(
            extractor
                .get("exhentai")
                .and_then(|value| value.get("cookies"))
                .and_then(|value| value.get("ipb_member_id"))
                .and_then(|value| value.as_str()),
            Some("member")
        );
    }

    #[test]
    fn e621_uses_api_categories_without_html_tag_fetching() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "e621".to_string(),
            url: "https://e621.net/posts?tags=canine".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let extractor = config
            .get("extractor")
            .and_then(|value| value.as_object())
            .expect("extractor config");
        assert!(extractor.get("e621").is_none());
    }

    #[test]
    fn deviantart_preserves_the_quality_authorized_by_the_signed_media_url() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "deviantart".to_string(),
            url: "https://www.deviantart.com/artist/gallery/".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert!(config["extractor"]["deviantart"]["quality"].is_null());
        assert_eq!(
            config["extractor"]["deviantart"]["picto-post-limit"].as_u64(),
            Some(1)
        );
        assert!(config["extractor"]["deviantart"]
            .get("picto-post-skip")
            .is_none());
        assert!(config["extractor"]["deviantart"].get("jwt").is_none());
        assert!(config["extractor"]["deviantart"].get("mature").is_none());
    }

    #[test]
    fn deviantart_uses_the_persisted_post_offset_for_continuation() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "deviantart".to_string(),
            url: "https://www.deviantart.com/artist/gallery/".to_string(),
            post_limit: Some(100),
            range_start: 101,
            source_cursor: Some("range:101".to_string()),
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let deviantart = &config["extractor"]["deviantart"];
        assert_eq!(deviantart["picto-post-limit"].as_u64(), Some(100));
        assert_eq!(deviantart["picto-post-skip"].as_u64(), Some(100));
    }

    #[test]
    fn rule34_uses_api_categories_and_one_second_pacing() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "rule34".to_string(),
            url: "https://rule34.xxx/index.php?page=post&s=list&tags=solo".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let extractor = config
            .get("extractor")
            .and_then(|value| value.as_object())
            .expect("extractor config");
        assert!(extractor.get("sleep-request").is_none());
        assert!(extractor.get("sleep").is_none());
        assert!(extractor.get("rule34").is_none());
    }

    #[test]
    fn artstation_does_not_cap_candidates_before_accepted_posts() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "artstation".to_string(),
            url: "https://www.artstation.com/artist".to_string(),
            post_limit: Some(2),
            range_start: 5,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert!(config["extractor"].get("artstation").is_none());
    }

    #[test]
    fn tumblr_uses_native_post_offset_and_keeps_embedded_media() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "tumblr".to_string(),
            url: "https://www.tumblr.com/nasa".to_string(),
            post_limit: Some(2),
            range_start: 5,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        let tumblr = &config["extractor"]["tumblr"];
        assert_eq!(tumblr["offset"].as_u64(), Some(4));
        assert_eq!(tumblr["picto-post-limit"].as_u64(), Some(2));
        assert_eq!(
            tumblr.get("reblogs").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert!(tumblr.get("inline").is_none());
        assert!(tumblr.get("external").is_none());
        assert!(tumblr.get("original").is_none());
    }

    #[test]
    fn furaffinity_receives_cookie_auth_under_its_extractor() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "furaffinity".to_string(),
            url: "https://www.furaffinity.net/gallery/artist".to_string(),
            post_limit: Some(2),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: Some(GalleryDlAuthConfig {
                site_category: "furaffinity".to_string(),
                fragment: serde_json::json!({
                    "cookies": {"a": "session-a", "b": "session-b"},
                }),
            }),
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert_eq!(
            config["extractor"]["furaffinity"]["cookies"]["a"].as_str(),
            Some("session-a")
        );
        assert_eq!(
            config["extractor"]["furaffinity"]["cookies"]["b"].as_str(),
            Some("session-b")
        );
    }

    #[test]
    fn idolcomplex_fetches_categorized_tags() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "idolcomplex".to_string(),
            url: "https://www.idolcomplex.com/en/posts?tags=solo".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert_eq!(
            config["extractor"]["idolcomplex"]["tags"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn sankaku_fetches_categorized_tags() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "sankaku".to_string(),
            url: "https://sankaku.app/?tags=solo+rating%3Asafe".to_string(),
            post_limit: Some(1),
            range_start: 1,
            source_cursor: Some("opaque-next".to_string()),
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert_eq!(config["extractor"]["sankaku"]["tags"].as_bool(), Some(true));
        assert_eq!(
            config["extractor"]["sankaku"]["picto-page-size"].as_u64(),
            Some(1)
        );
        assert_eq!(
            config["extractor"]["sankaku"]["picto-next"].as_str(),
            Some("opaque-next")
        );
    }

    #[test]
    fn patreon_resumes_from_its_persisted_api_cursor() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "patreon".to_string(),
            url: "https://www.patreon.com/creator/posts".to_string(),
            post_limit: Some(100),
            range_start: 1,
            source_cursor: Some("next-page".to_string()),
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert_eq!(
            config["extractor"]["patreon"]["cursor"].as_str(),
            Some("next-page")
        );
        assert_eq!(
            config["extractor"]["patreon"]["picto-post-limit"].as_u64(),
            Some(100)
        );
        assert!(config["extractor"]["patreon"]
            .get("picto-post-skip")
            .is_none());
    }

    #[test]
    fn patreon_first_page_resume_marker_is_not_sent_to_the_api() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "patreon".to_string(),
            url: "https://www.patreon.com/creator/posts".to_string(),
            post_limit: Some(2),
            range_start: 1,
            source_cursor: Some("patreon:first-page".to_string()),
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert!(config["extractor"]["patreon"].get("cursor").is_none());
    }

    #[test]
    fn patreon_legacy_query_skips_its_already_completed_first_batch() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "patreon".to_string(),
            url: "https://www.patreon.com/creator/posts".to_string(),
            post_limit: Some(100),
            range_start: 101,
            source_cursor: None,
            abort_threshold: None,
            auth: None,
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            pacing_state_dir: std::path::PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
        assert_eq!(
            config["extractor"]["patreon"]["picto-post-skip"].as_u64(),
            Some(100)
        );
    }
}
