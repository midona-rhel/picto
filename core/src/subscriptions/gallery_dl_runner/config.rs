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
    // Without this, booru sites only provide a flat `tags` string.
    extractor.insert("tags".into(), serde_json::Value::Bool(true));

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

    serde_json::Value::Object(root)
}

fn apply_credential_auth(
    extractor: &mut serde_json::Map<String, serde_json::Value>,
    cred: &SiteCredential,
) {
    let auth = crate::credential_store::build_extractor_auth(cred);
    extractor.insert(cred.site_category.clone(), auth);
}
