use std::path::Path;

use crate::credential_store::SiteCredential;

use super::RunOptions;

pub fn build_config(opts: &RunOptions, _temp_dir: &Path) -> serde_json::Value {
    let mut extractor = serde_json::Map::new();

    extractor.insert(
        "sleep-request".into(),
        serde_json::Value::Number(
            serde_json::Number::from_f64(opts.sleep_request).unwrap_or(serde_json::Number::from(2)),
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

    if let Some(ref cred) = opts.credential {
        apply_credential_auth(&mut extractor, cred);
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
    cred: &SiteCredential,
) {
    let auth = crate::credential_store::build_extractor_auth(cred);
    extractor.insert(cred.site_category.clone(), auth);
}
