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

/// How pixel values should be normalized before inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum PixelNorm {
    /// [0, 255] → [0.0, 1.0]  (standard Keras / torchvision)
    ZeroOne,
    /// [0, 255] → [-1.0, 1.0]  (SmilingWolf WD14)
    NegOneOne,
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
    /// Pixel normalization range.
    pub pixel_norm: PixelNorm,
}

/// Static registry of known models.
///
/// WD14: SmilingWolf's ViT-Large v3 — highest F1 score (0.4674) among all v3 variants.
/// E621: Z3D-E621-ConvNext — community-trained E621 tagger in ONNX format.
pub fn known_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            slug: "wd14-swinv2-v3".into(),
            label: "WD14 SwinV2 v3".into(),
            onnx_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/model.onnx".into(),
            labels_url: "https://huggingface.co/SmilingWolf/wd-swinv2-tagger-v3/resolve/main/selected_tags.csv".into(),
            input_size: 448,
            channel_order: ChannelOrder::Bgr,
            pixel_norm: PixelNorm::NegOneOne,
        },
        ModelInfo {
            slug: "z3d-e621-convnext".into(),
            label: "E621 ConvNext (Z3D)".into(),
            onnx_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/model.onnx".into(),
            labels_url: "https://huggingface.co/toynya/Z3D-E621-Convnext/resolve/main/tags-selected.csv".into(),
            input_size: 448,
            channel_order: ChannelOrder::Rgb,
            pixel_norm: PixelNorm::ZeroOne,
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
    let has_labels = dir.join("selected_tags.csv").exists()
        || dir.join("tags-selected.csv").exists();
    has_onnx && has_labels
}
