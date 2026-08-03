use std::path::Path;

use crate::subscriptions::credential_service::GalleryDlAuthConfig;

use super::RunOptions;

pub fn build_config(opts: &RunOptions, _temp_dir: &Path) -> serde_json::Value {
    let mut extractor = serde_json::Map::new();

    extractor.insert(
        "sleep-request".into(),
        serde_json::Value::Number(
            serde_json::Number::from_f64(opts.sleep_request).unwrap_or(serde_json::Number::from(2)),
        ),
    );

    // Delay between FILE downloads (sleep-request only paces page/API requests).
    // Without this, bulk runs fire hundreds of media downloads back-to-back and
    // CDNs (gelbooru especially) start refusing until retries are exhausted —
    // observed as mass "gallery-dl exhausted item retries" failures.
    extractor.insert(
        "sleep".into(),
        serde_json::Value::Number(
            serde_json::Number::from_f64(1.0).unwrap_or(serde_json::Number::from(1)),
        ),
    );

    extractor.insert("metadata".into(), serde_json::Value::Bool(true));

    // Fetch categorized tags (tags_artist, tags_character, etc.) from post HTML.
    // Only for booru sites that support it — enabling globally causes extra
    // HTTP requests on sites like Kemono/Coomer that don't have tag categories.
    let tags_true = serde_json::Value::Bool(true);
    for booru in [
        "gelbooru",
        "danbooru",
        "rule34",
        "safebooru",
        "yandere",
        "konachan",
        "sankaku",
        "idolcomplex",
        "3dbooru",
    ] {
        let site_obj = extractor
            .entry(booru)
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let serde_json::Value::Object(ref mut m) = site_obj {
            m.insert("tags".into(), tags_true.clone());
        }
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

    // Sites using dispatch URLs: include gallery + scraps, exclude favorites/stories.
    let gallery_scraps = serde_json::Value::Array(vec![
        serde_json::Value::String("gallery".into()),
        serde_json::Value::String("scraps".into()),
    ]);
    for site_key in ["furaffinity", "hentaifoundry"] {
        let site_obj = extractor
            .entry(site_key)
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let serde_json::Value::Object(ref mut m) = site_obj {
            m.insert("include".into(), gallery_scraps.clone());
        }
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
    let site_obj = extractor
        .entry(auth.site_category.clone())
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
    use crate::subscriptions::credential_service::GalleryDlAuthConfig;

    use super::build_config;
    use super::RunOptions;

    #[test]
    fn build_config_keeps_booru_tags_when_api_credentials_are_present() {
        let opts = RunOptions {
            subscription_id: Some(1),
            query_id: Some(2),
            site_id: "gelbooru".to_string(),
            url: "https://gelbooru.com/index.php?page=post&s=list&tags=test".to_string(),
            post_limit: Some(1),
            range_start: 1,
            abort_threshold: None,
            sleep_request: 2.0,
            auth: Some(GalleryDlAuthConfig {
                site_category: "gelbooru".to_string(),
                fragment: serde_json::json!({
                    "api-key": "secret",
                    "user-id": "277923",
                }),
            }),
            archive_path: std::path::PathBuf::new(),
            archive_prefix: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        let config = build_config(&opts, std::path::Path::new("/tmp"));
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
}
