//! ONNX inference engine for tagger models.
//!
//! Manages the ONNX Runtime session lifecycle and runs image classification
//! to produce tag predictions with confidence scores.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::labels::LabelEntry;
use super::models::ChannelOrder;

/// A single tag prediction from inference.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct TagPrediction {
    /// Tag name (e.g. `"1girl"`).
    pub tag: String,
    /// Picto namespace (e.g. `"general"`, `"character"`).
    pub namespace: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Slug of the model that produced this prediction.
    pub model: String,
}

/// Per-category confidence thresholds.
#[derive(Debug, Clone)]
pub struct Thresholds {
    pub general: f32,
    pub character: f32,
    pub copyright: f32,
    pub artist: f32,
    pub species: f32,
    pub rating: f32,
}

impl Thresholds {
    fn for_namespace(&self, namespace: &str) -> f32 {
        match namespace {
            "general" => self.general,
            "character" => self.character,
            "copyright" => self.copyright,
            "artist" => self.artist,
            "species" => self.species,
            "rating" => self.rating,
            _ => self.general,
        }
    }
}

/// An active tagger session wrapping an ONNX Runtime session + parsed labels.
pub struct TaggerSession {
    session: ort::session::Session,
    labels: Vec<LabelEntry>,
    input_size: u32,
    channel_order: ChannelOrder,
    slug: String,
}

impl TaggerSession {
    /// Create a new tagger session, loading the ONNX model and labels from disk.
    pub fn load(
        model_dir: &Path,
        slug: &str,
        input_size: u32,
        channel_order: ChannelOrder,
    ) -> Result<Self, String> {
        let model_path = model_dir.join("model.onnx");
        // Labels CSV may be named differently per model
        let labels_path = ["selected_tags.csv", "tags-selected.csv"]
            .iter()
            .map(|name| model_dir.join(name))
            .find(|p| p.exists())
            .ok_or_else(|| format!("Labels CSV not found in {}", model_dir.display()))?;

        if !model_path.exists() {
            return Err(format!("Model file not found: {}", model_path.display()));
        }

        let labels = super::labels::parse_labels_csv(&labels_path)?;
        let session = create_session(&model_path)?;

        tracing::info!(slug, labels = labels.len(), "AI tagger session loaded");

        Ok(Self {
            session,
            labels,
            input_size,
            channel_order,
            slug: slug.to_string(),
        })
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn gpu_backend(&self) -> String {
        "CPU".into()
    }

    /// Run inference on raw image bytes and return predictions above thresholds.
    pub fn predict(
        &mut self,
        image_bytes: &[u8],
        thresholds: &Thresholds,
    ) -> Result<Vec<TagPrediction>, String> {
        let tensor = preprocess_image(image_bytes, self.input_size, self.channel_order)?;

        let input_value = ort::value::Tensor::from_array(tensor)
            .map_err(|e| format!("Failed to create input tensor: {e}"))?;

        // Log model input/output names for debugging
        tracing::debug!(
            inputs = ?self.session.inputs().iter().map(|i| i.name()).collect::<Vec<_>>(),
            outputs = ?self.session.outputs().iter().map(|o| o.name()).collect::<Vec<_>>(),
            "Model I/O names"
        );

        let outputs = self
            .session
            .run(ort::inputs![input_value])
            .map_err(|e| format!("Inference failed: {e}"))?;

        // Model outputs a single tensor of shape [1, num_labels]
        let (_, logits) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract output tensor: {e}"))?;

        tracing::info!(
            slug = self.slug,
            output_len = logits.len(),
            labels_len = self.labels.len(),
            "Inference complete, processing output"
        );

        // Log a few sample values to diagnose sigmoid vs raw probability
        if logits.len() >= 5 {
            let sample: Vec<f32> = logits.iter().take(5).copied().collect();
            let sample_sigmoid: Vec<f32> = sample.iter().map(|&v| sigmoid(v)).collect();
            tracing::debug!(
                ?sample,
                ?sample_sigmoid,
                "First 5 raw outputs and their sigmoid values"
            );
        }

        // Check if outputs are already probabilities (0-1 range) or logits (need sigmoid)
        let max_val = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min_val = logits.iter().copied().fold(f32::INFINITY, f32::min);
        let outputs_are_probabilities = min_val >= 0.0 && max_val <= 1.0;

        tracing::info!(
            min_val,
            max_val,
            outputs_are_probabilities,
            "Output value range"
        );

        let mut predictions = Vec::new();
        for (i, &logit) in logits.iter().enumerate() {
            if i >= self.labels.len() {
                break;
            }
            // Use sigmoid only if outputs are logits; skip if already probabilities
            let confidence = if outputs_are_probabilities {
                logit
            } else {
                sigmoid(logit)
            };
            let label = &self.labels[i];
            let threshold = thresholds.for_namespace(&label.namespace);

            if confidence >= threshold {
                predictions.push(TagPrediction {
                    tag: label.name.clone(),
                    namespace: label.namespace.clone(),
                    confidence,
                    model: self.slug.clone(),
                });
            }
        }

        predictions.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(predictions)
    }
}

/// Create an ONNX Runtime session (CPU execution provider).
///
/// GPU acceleration (CoreML, CUDA, DirectML) requires models that are
/// compatible with those providers. WD14/E621 models currently use
/// operators that fail on CoreML, so we use CPU for now.
fn create_session(model_path: &Path) -> Result<ort::session::Session, String> {
    tracing::info!(path = %model_path.display(), "Loading ONNX model");
    ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(model_path))
        .map_err(|e| format!("Failed to load ONNX model: {e}"))
}

/// Preprocess raw image bytes into a float32 tensor of shape [1, H, W, 3] (NHWC).
///
/// Steps:
/// 1. Decode image
/// 2. Pad to square with white background (preserving aspect ratio)
/// 3. Resize to model input size
/// 4. Convert to float, normalize to [-1, 1] via `pixel / 127.5 - 1.0`
/// 5. Reorder channels if model expects BGR
fn preprocess_image(
    image_bytes: &[u8],
    input_size: u32,
    channel_order: ChannelOrder,
) -> Result<ndarray::Array4<f32>, String> {
    let img =
        image::load_from_memory(image_bytes).map_err(|e| format!("Failed to decode image: {e}"))?;

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());

    // Pad to square with white background
    let max_dim = w.max(h);
    let mut padded = image::RgbImage::from_pixel(max_dim, max_dim, image::Rgb([255, 255, 255]));
    let x_offset = (max_dim - w) / 2;
    let y_offset = (max_dim - h) / 2;
    image::imageops::overlay(&mut padded, &rgb, x_offset as i64, y_offset as i64);

    // Resize to model input size
    let resized = image::imageops::resize(
        &padded,
        input_size,
        input_size,
        image::imageops::FilterType::Lanczos3,
    );

    let size = input_size as usize;

    // NHWC: [batch, height, width, channels], normalized to [-1, 1]
    let mut tensor = ndarray::Array4::<f32>::zeros((1, size, size, 3));
    for y in 0..size {
        for x in 0..size {
            let pixel = resized.get_pixel(x as u32, y as u32);
            let (c0, c1, c2) = match channel_order {
                ChannelOrder::Rgb => (pixel[0], pixel[1], pixel[2]),
                ChannelOrder::Bgr => (pixel[2], pixel[1], pixel[0]),
            };
            // Raw float32 [0, 255] — no normalization (matches ComfyUI reference)
            tensor[[0, y, x, 0]] = c0 as f32;
            tensor[[0, y, x, 1]] = c1 as f32;
            tensor[[0, y, x, 2]] = c2 as f32;
        }
    }

    Ok(tensor)
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Map of model slug → loaded TaggerSession, behind an async mutex.
pub type SharedTaggerSessions =
    Arc<tokio::sync::Mutex<std::collections::HashMap<String, TaggerSession>>>;

/// Create a new empty sessions map.
pub fn new_shared_sessions() -> SharedTaggerSessions {
    Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()))
}
