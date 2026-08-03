//! Known tagger model registry.
//!
//! Each entry describes a model available for download from Hugging Face,
//! its expected input dimensions, and how its label CSV maps category
//! integers to Picto tag namespaces.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Channel ordering expected by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "lowercase")]
pub enum ChannelOrder {
    Rgb,
    Bgr,
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
    /// URL to the `selected_tags.csv` label file.
    pub labels_url: String,
    /// Model input image size (width = height).
    #[ts(type = "number")]
    pub input_size: u32,
    /// Channel order expected by the model.
    pub channel_order: ChannelOrder,
    /// Approximate download size of the ONNX file in bytes.
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// Which tag vocabulary the model produces (shown in the settings panel).
    pub dataset: String,
    /// Heavy models trade speed for accuracy and are only recommended on
    /// machines with plenty of memory.
    pub heavy: bool,
}

/// Static registry of known models.
///
/// WD14 SwinV2 v3 + Z3D E621 ConvNext are the recommended default pair;
/// EVA02-Large v3 is the highest-accuracy WD variant but several times
/// slower and ~1.3 GB on disk, so it is marked heavy.
pub fn known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            slug: "wd14-swinv2-v3".into(),
            label: "WD14 SwinV2 v3".into(),
            onnx_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/model.onnx".into(),
            labels_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/selected_tags.csv".into(),
            input_size: 448,
            channel_order: ChannelOrder::Bgr,
            size_bytes: 467_000_000,
            dataset: "Danbooru tags".into(),
            heavy: false,
        },
        ModelInfo {
            slug: "z3d-e621-convnext".into(),
            label: "E621 ConvNext (Z3D)".into(),
            onnx_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/model.onnx".into(),
            labels_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/tags-selected.csv".into(),
            input_size: 448,
            channel_order: ChannelOrder::Rgb,
            size_bytes: 390_000_000,
            dataset: "e621 tags".into(),
            heavy: false,
        },
        ModelInfo {
            slug: "wd14-eva02-large-v3".into(),
            label: "WD14 EVA02-Large v3".into(),
            onnx_url: "https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3/resolve/main/model.onnx".into(),
            labels_url: "https://huggingface.co/SmilingWolf/wd-eva02-large-tagger-v3/resolve/main/selected_tags.csv".into(),
            input_size: 448,
            channel_order: ChannelOrder::Bgr,
            size_bytes: 1_260_000_000,
            dataset: "Danbooru tags, highest accuracy".into(),
            heavy: true,
        },
    ]
}

/// Look up a model by slug.
pub fn find_model(slug: &str) -> Option<ModelInfo> {
    known_models().into_iter().find(|m| m.slug == slug)
}

/// Return the on-disk directory for a model's files.
pub fn model_dir(models_root: &std::path::Path, slug: &str) -> std::path::PathBuf {
    models_root.join(slug)
}

/// Check whether a model's ONNX file and labels CSV both exist on disk.
pub fn is_model_downloaded(models_root: &std::path::Path, slug: &str) -> bool {
    let dir = model_dir(models_root, slug);
    let has_onnx = dir.join("model.onnx").exists();
    let has_labels =
        dir.join("selected_tags.csv").exists() || dir.join("tags-selected.csv").exists();
    has_onnx && has_labels
}
