//! Known tagger model registry.
//!
//! Each entry describes a model available for download from Hugging Face,
//! its expected input dimensions, and how its label CSV maps category
//! integers to Picto tag namespaces.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use ts_rs::TS;

pub(crate) const BUNDLE_MARKER: &str = ".bundle-validated";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CoreMlArtifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct CoreMlArtifactRegistry {
    assets: std::collections::HashMap<String, CoreMlArtifact>,
}

/// Channel ordering expected by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "lowercase")]
pub enum ChannelOrder {
    Rgb,
    Bgr,
}

/// Interpretation of each model output value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputActivation {
    Probability,
    Logit,
}

impl Default for OutputActivation {
    fn default() -> Self {
        Self::Probability
    }
}

/// A known tagger model that can be downloaded and used for inference.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ModelInfo {
    /// Slug used in settings and on-disk path, e.g. `"wd14-swinv2-v3"`.
    pub slug: String,
    /// Human-readable label shown in the settings panel.
    pub label: String,
    /// URL to the ONNX model file on Hugging Face.
    pub onnx_url: String,
    /// SHA-256 of the registered ONNX artifact.
    #[serde(skip)]
    #[ts(skip)]
    pub onnx_sha256: String,
    /// URL to the `selected_tags.csv` label file.
    pub labels_url: String,
    /// SHA-256 of the registered label artifact.
    #[serde(skip)]
    #[ts(skip)]
    pub labels_sha256: String,
    /// Model input image size (width = height).
    #[ts(type = "number")]
    pub input_size: u32,
    /// Channel order expected by the model.
    pub channel_order: ChannelOrder,
    /// Whether inference outputs are probabilities or logits.
    #[serde(skip)]
    #[ts(skip)]
    pub output_activation: OutputActivation,
    /// Approximate download size of the ONNX file in bytes.
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// Which tag vocabulary the model produces (shown in the settings panel).
    pub dataset: String,
    /// Heavy models trade speed for accuracy and are only recommended on
    /// machines with plenty of memory.
    pub heavy: bool,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) coreml: Option<CoreMlArtifact>,
}

/// Static registry of known models.
///
/// WD14 SwinV2 v3 + Z3D E621 ConvNext are the recommended default pair;
/// EVA02-Large v3 is the highest-accuracy WD variant but several times
/// slower and ~1.3 GB on disk, so it is marked heavy.
pub fn known_models() -> Vec<ModelInfo> {
    let mut coreml = serde_json::from_str::<CoreMlArtifactRegistry>(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/ai/coreml-artifacts.json"
    )))
    .expect("Core ML artifact registry must be valid")
    .assets;
    vec![
        ModelInfo {
            slug: "wd14-swinv2-v3".into(),
            label: "WD14 SwinV2 v3".into(),
            onnx_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/model.onnx".into(),
            onnx_sha256: "e6774bff34d43bd49f75a47db4ef217dce701c9847b546523eb85ff6dbba1db1".into(),
            labels_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/selected_tags.csv".into(),
            labels_sha256: "298633d94d0031d2081c0893f29c82eab7f0df00b08483ba8f29d1e979441217".into(),
            input_size: 448,
            channel_order: ChannelOrder::Bgr,
            output_activation: OutputActivation::Probability,
            size_bytes: 467_000_000,
            dataset: "Danbooru · general, character, rating".into(),
            heavy: false,
            coreml: coreml.remove("wd14-swinv2-v3"),
        },
        ModelInfo {
            slug: "z3d-e621-convnext".into(),
            label: "E621 ConvNext (Z3D)".into(),
            onnx_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/model.onnx".into(),
            onnx_sha256: "672f6c1b987abfdb311c41ecd57efdc0e5b1860944a3722984326316f4655c70".into(),
            labels_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/tags-selected.csv".into(),
            labels_sha256: "609c75136b90fc0a87cce111961f172d029ab400299cfa8bbf6830918305aa40".into(),
            input_size: 448,
            // Z3D's reference preprocessing consumes BGR tensors.
            channel_order: ChannelOrder::Bgr,
            output_activation: OutputActivation::Probability,
            size_bytes: 390_000_000,
            dataset: "e621 · general, creator, series, character, species".into(),
            heavy: false,
            coreml: coreml.remove("z3d-e621-convnext"),
        },
        ModelInfo {
            slug: "wd14-eva02-large-v3".into(),
            label: "WD14 EVA02-Large v3".into(),
            onnx_url: "https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3/resolve/main/model.onnx".into(),
            onnx_sha256: "9e768793060c7939b277ccb382783e8670e8a042d29d77aa736be0c8cc898bfc".into(),
            labels_url: "https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3/resolve/main/selected_tags.csv".into(),
            labels_sha256: "298633d94d0031d2081c0893f29c82eab7f0df00b08483ba8f29d1e979441217".into(),
            input_size: 448,
            channel_order: ChannelOrder::Bgr,
            output_activation: OutputActivation::Probability,
            size_bytes: 1_260_000_000,
            dataset: "Danbooru · general, character, rating · Highest accuracy".into(),
            heavy: true,
            coreml: coreml.remove("wd14-eva02-large-v3"),
        },
    ]
}

/// Look up a model by slug.
pub fn find_model(slug: &str) -> Option<ModelInfo> {
    known_models().into_iter().find(|m| m.slug == slug)
}

/// Return the on-disk directory for a registry-owned model.
pub fn model_dir(models_root: &std::path::Path, model: &ModelInfo) -> std::path::PathBuf {
    models_root.join(&model.slug)
}

/// Check whether a known model has a complete bundle activated by Picto.
pub fn is_model_downloaded(models_root: &std::path::Path, slug: &str) -> bool {
    let Some(model) = find_model(slug) else {
        return false;
    };
    let dir = model_dir(models_root, &model);
    bundle_is_marked(&dir, &model)
}

fn bundle_marker_content(model: &ModelInfo) -> String {
    let mut marker = format!(
        "picto-ai-model-bundle-v3\nmodel={}\nlabels={}\n",
        model.onnx_sha256, model.labels_sha256
    );
    #[cfg(target_os = "macos")]
    if let Some(artifact) = &model.coreml {
        marker.push_str(&format!("coreml={}\n", artifact.sha256));
    }
    marker
}

pub(crate) fn bundle_is_marked(dir: &std::path::Path, model: &ModelInfo) -> bool {
    let portable = dir.join("model.onnx").is_file()
        && dir.join("selected_tags.csv").is_file()
        && std::fs::read_to_string(dir.join(BUNDLE_MARKER))
            .ok()
            .as_deref()
            == Some(bundle_marker_content(model).as_str());
    #[cfg(target_os = "macos")]
    return portable
        && model
            .coreml
            .as_ref()
            .is_none_or(|_| dir.join("model.mlpackage").is_dir());
    #[cfg(not(target_os = "macos"))]
    portable
}

pub(crate) fn mark_bundle_validated(
    dir: &std::path::Path,
    model: &ModelInfo,
) -> Result<(), String> {
    std::fs::write(dir.join(BUNDLE_MARKER), bundle_marker_content(model))
        .map_err(|e| format!("Failed to write model validation marker: {e}"))
}

pub(crate) fn validate_bundle_integrity(
    dir: &std::path::Path,
    model: &ModelInfo,
) -> Result<(), String> {
    if !bundle_is_marked(dir, model) {
        return Err(format!("Model bundle '{}' is incomplete", model.slug));
    }
    for (path, expected) in [
        (dir.join("model.onnx"), model.onnx_sha256.as_str()),
        (dir.join("selected_tags.csv"), model.labels_sha256.as_str()),
    ] {
        let mut file = std::fs::File::open(&path)
            .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 128 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(format!(
                "Model bundle '{}' failed integrity validation for {}",
                model.slug,
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn add_platform_artifact(dir: &std::path::Path) {
        #[cfg(target_os = "macos")]
        std::fs::create_dir_all(dir.join("model.mlpackage")).unwrap();
    }

    #[test]
    fn unknown_slugs_are_not_downloaded() {
        let root = TempDir::new().unwrap();
        let unknown = "../outside";

        assert!(!is_model_downloaded(root.path(), unknown));
    }

    #[test]
    fn bare_files_are_not_reported_as_downloaded() {
        let root = TempDir::new().unwrap();
        let model = find_model("wd14-swinv2-v3").unwrap();
        let dir = model_dir(root.path(), &model);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model.onnx"), b"not validated").unwrap();
        std::fs::write(
            dir.join("selected_tags.csv"),
            b"tag_id,name,category,count\n0,tag,0,1\n",
        )
        .unwrap();
        add_platform_artifact(&dir);

        assert!(!is_model_downloaded(root.path(), "wd14-swinv2-v3"));
        mark_bundle_validated(&dir, &model).unwrap();
        assert!(is_model_downloaded(root.path(), "wd14-swinv2-v3"));
    }

    #[test]
    fn z3d_uses_bgr_channel_order() {
        assert_eq!(
            find_model("z3d-e621-convnext").unwrap().channel_order,
            ChannelOrder::Bgr
        );
    }

    #[test]
    fn registered_artifact_hashes_are_sha256_values() {
        for model in known_models() {
            for checksum in [&model.onnx_sha256, &model.labels_sha256] {
                assert_eq!(checksum.len(), 64, "{} has an invalid checksum", model.slug);
                assert!(
                    checksum.bytes().all(|byte| byte.is_ascii_hexdigit()),
                    "{} has a non-hex checksum",
                    model.slug
                );
            }
        }
    }

    #[test]
    fn bundle_integrity_detects_artifact_corruption() {
        let root = TempDir::new().unwrap();
        let mut model = find_model("wd14-swinv2-v3").unwrap();
        let dir = model_dir(root.path(), &model);
        std::fs::create_dir_all(&dir).unwrap();
        let onnx = b"test model";
        let labels = b"tag_id,name,category,count\n0,tag,0,1\n";
        model.onnx_sha256 = hex::encode(Sha256::digest(onnx));
        model.labels_sha256 = hex::encode(Sha256::digest(labels));
        std::fs::write(dir.join("model.onnx"), onnx).unwrap();
        std::fs::write(dir.join("selected_tags.csv"), labels).unwrap();
        add_platform_artifact(&dir);
        mark_bundle_validated(&dir, &model).unwrap();

        validate_bundle_integrity(&dir, &model).unwrap();
        std::fs::write(dir.join("model.onnx"), b"corrupt").unwrap();
        assert!(validate_bundle_integrity(&dir, &model).is_err());
    }
}
