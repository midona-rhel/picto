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

    let mut root = serde_json::Map::new();
    root.insert("extractor".into(), serde_json::Value::Object(extractor));

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
