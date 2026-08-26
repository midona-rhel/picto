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
pub(crate) const COREML_MARKER: &str = ".coreml-artifact";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RegisteredArtifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ArtifactRegistry {
    assets: std::collections::HashMap<String, RegisteredArtifact>,
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

/// Model-specific input and label contract retained by the portable runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModelAdapter {
    #[default]
    Wd,
    OppaiOracle,
    DanbooruTagQuery,
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
    #[serde(skip)]
    #[ts(skip)]
    pub adapter: ModelAdapter,
    /// Approximate download size of the ONNX file in bytes.
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// Which tag vocabulary the model produces (shown in the settings panel).
    pub dataset: String,
    /// Verified warm single-image inference latency on the reference Mac.
    pub reference_inference_ms: f32,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) label_categories: Option<RegisteredArtifact>,
    /// Heavy models trade speed for accuracy and are only recommended on
    /// machines with plenty of memory.
    pub heavy: bool,
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) coreml: Option<RegisteredArtifact>,
}

/// Static registry of known models.
///
/// WD14 SwinV2 v3 + Z3D E621 ConvNext are the recommended default pair;
/// EVA02-Large v3 is the highest-accuracy WD variant but several times
/// slower and ~1.3 GB on disk, so it is marked heavy.
pub fn known_models() -> Vec<ModelInfo> {
    let mut coreml = serde_json::from_str::<ArtifactRegistry>(include_str!(concat!(
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
            adapter: ModelAdapter::Wd,
            size_bytes: 467_000_000,
            dataset: "Danbooru · general, character, rating".into(),
            reference_inference_ms: 17.74,
            label_categories: None,
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
            adapter: ModelAdapter::Wd,
            size_bytes: 390_000_000,
            dataset: "e621 · general, creator, series, character, species".into(),
            reference_inference_ms: 14.71,
            label_categories: None,
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
            adapter: ModelAdapter::Wd,
            size_bytes: 1_260_000_000,
            dataset: "Danbooru · general, character, rating · Highest accuracy".into(),
            reference_inference_ms: 48.38,
            label_categories: None,
            heavy: true,
            coreml: coreml.remove("wd14-eva02-large-v3"),
        },
        ModelInfo {
            slug: "oppai-oracle-v1-1".into(),
            label: "OppaiOracle V1.1".into(),
            onnx_url: "https://huggingface.co/Grio43/OppaiOracle/resolve/96992fa30568c386e9fe7c8a1a68f798a3202c09/V1.1_onnx/model.onnx".into(),
            onnx_sha256: "8567852deb135eccfe4b8445d48e4476ee8846436486679adc0642cfeda07d13".into(),
            labels_url: "https://huggingface.co/Grio43/OppaiOracle/resolve/96992fa30568c386e9fe7c8a1a68f798a3202c09/V1.1_onnx/selected_tags.csv".into(),
            labels_sha256: "6e51d4a4023d52e5b6485123be113515ba70d137150d7976168f4d5b243e5da8".into(),
            input_size: 448,
            channel_order: ChannelOrder::Rgb,
            output_activation: OutputActivation::Probability,
            adapter: ModelAdapter::OppaiOracle,
            size_bytes: 993_246_982,
            dataset: "Danbooru · wide-vocabulary general tags".into(),
            reference_inference_ms: 86.45,
            label_categories: None,
            heavy: true,
            coreml: coreml.remove("oppai-oracle-v1-1"),
        },
        ModelInfo {
            slug: "danbooru-tag-query-b16".into(),
            label: "DanbooruTagQuery B16".into(),
            onnx_url: "https://huggingface.co/realphongha/danbooru-tag-query/resolve/d2f8da27a23db4adf95c3e663af183b92784ad3e/models/DanbooruTagQuery_b16_448x448/model.onnx".into(),
            onnx_sha256: "aef21a2a04605ba1b059e6a14471dbc6fec7589753afb92f352336ef60caa987".into(),
            labels_url: "https://huggingface.co/realphongha/danbooru-tag-query/resolve/d2f8da27a23db4adf95c3e663af183b92784ad3e/models/DanbooruTagQuery_b16_448x448/tag_to_id.json".into(),
            labels_sha256: "00959521d85ff61574d6cd35ac909dc67cf68cb65ba6267cf27ff64785ee5935".into(),
            input_size: 448,
            channel_order: ChannelOrder::Rgb,
            output_activation: OutputActivation::Logit,
            adapter: ModelAdapter::DanbooruTagQuery,
            size_bytes: 387_406_778,
            dataset: "Danbooru · general, character, series".into(),
            reference_inference_ms: 13.23,
            label_categories: Some(RegisteredArtifact {
                url: "https://huggingface.co/realphongha/danbooru-tag-query/resolve/d2f8da27a23db4adf95c3e663af183b92784ad3e/models/DanbooruTagQuery_b16_448x448/tag_category.json".into(),
                sha256: "32490933a3054325a6c9d941123fc5ace3fe1f535826756083ce201ef243b396".into(),
                size: 246_458,
            }),
            heavy: false,
            coreml: coreml.remove("danbooru-tag-query-b16"),
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

pub fn optimization_supported(models_root: &std::path::Path, slug: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = models_root;
        let Some(model) = find_model(slug) else {
            return false;
        };
        return model.coreml.is_some();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (models_root, slug);
        false
    }
}

pub fn is_model_optimized(models_root: &std::path::Path, slug: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(model) = find_model(slug) else {
            return false;
        };
        return coreml_artifact_is_current(&model_dir(models_root, &model), &model);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (models_root, slug);
        false
    }
}

pub(crate) fn coreml_artifact_is_current(dir: &std::path::Path, model: &ModelInfo) -> bool {
    let Some(artifact) = &model.coreml else {
        return false;
    };
    dir.join("model.mlmodelc").is_dir()
        && std::fs::read_to_string(dir.join(COREML_MARKER))
            .ok()
            .as_deref()
            == Some(artifact.sha256.as_str())
}

pub(crate) fn mark_coreml_artifact_current(
    dir: &std::path::Path,
    model: &ModelInfo,
) -> Result<(), String> {
    let artifact = model
        .coreml
        .as_ref()
        .ok_or_else(|| format!("Model '{}' has no Core ML artifact", model.slug))?;
    std::fs::write(dir.join(COREML_MARKER), &artifact.sha256)
        .map_err(|error| format!("Failed to mark Core ML artifact as current: {error}"))
}

fn bundle_marker_content(model: &ModelInfo) -> String {
    let mut marker = format!(
        "picto-ai-model-bundle-v3\nmodel={}\nlabels={}\n",
        model.onnx_sha256, model.labels_sha256
    );
    if let Some(categories) = &model.label_categories {
        marker.push_str(&format!("label_categories={}\n", categories.sha256));
    }
    marker
}

pub(crate) fn bundle_is_marked(dir: &std::path::Path, model: &ModelInfo) -> bool {
    let portable = dir.join("model.onnx").is_file()
        && dir.join("selected_tags.csv").is_file()
        && model
            .label_categories
            .as_ref()
            .is_none_or(|_| dir.join("label-categories.json").is_file())
        && std::fs::read_to_string(dir.join(BUNDLE_MARKER))
            .ok()
            .as_deref()
            == Some(bundle_marker_content(model).as_str());
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
    let mut artifacts = vec![
        (dir.join("model.onnx"), model.onnx_sha256.as_str()),
        (dir.join("selected_tags.csv"), model.labels_sha256.as_str()),
    ];
    if let Some(categories) = &model.label_categories {
        artifacts.push((
            dir.join("label-categories.json"),
            categories.sha256.as_str(),
        ));
    }
    for (path, expected) in artifacts {
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

    #[test]
    fn unknown_slugs_are_not_downloaded() {
        let root = TempDir::new().unwrap();
        let unknown = "../outside";

        assert!(!is_model_downloaded(root.path(), unknown));
    }

    #[test]
    fn product_registry_contains_every_validated_tagger() {
        let models = known_models();
        assert_eq!(models.len(), 5);
        assert!(models.iter().any(|model| {
            model.slug == "oppai-oracle-v1-1"
                && model.adapter == ModelAdapter::OppaiOracle
                && model.reference_inference_ms == 86.45
                && model.coreml.is_some()
        }));
        assert!(models.iter().any(|model| {
            model.slug == "danbooru-tag-query-b16"
                && model.adapter == ModelAdapter::DanbooruTagQuery
                && model.reference_inference_ms == 13.23
                && model.label_categories.is_some()
        }));
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
        assert!(!is_model_downloaded(root.path(), "wd14-swinv2-v3"));
        mark_bundle_validated(&dir, &model).unwrap();
        assert!(is_model_downloaded(root.path(), "wd14-swinv2-v3"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn optimized_status_requires_the_registered_artifact_checksum() {
        let root = TempDir::new().unwrap();
        let model = find_model("oppai-oracle-v1-1").unwrap();
        let dir = model_dir(root.path(), &model);
        std::fs::create_dir_all(dir.join("model.mlmodelc")).unwrap();

        assert!(!is_model_optimized(root.path(), &model.slug));
        std::fs::write(dir.join(COREML_MARKER), "stale-v1").unwrap();
        assert!(!is_model_optimized(root.path(), &model.slug));
        mark_coreml_artifact_current(&dir, &model).unwrap();
        assert!(is_model_optimized(root.path(), &model.slug));
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
        mark_bundle_validated(&dir, &model).unwrap();

        validate_bundle_integrity(&dir, &model).unwrap();
        std::fs::write(dir.join("model.onnx"), b"corrupt").unwrap();
        assert!(validate_bundle_integrity(&dir, &model).is_err());
    }
}
