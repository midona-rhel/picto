//! ONNX inference engine for tagger models.
//!
//! Manages the ONNX Runtime session lifecycle and runs image classification
//! to produce tag predictions with confidence scores.

use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::labels::LabelEntry;
use super::models::{ChannelOrder, OutputActivation};

const BELOW_THRESHOLD_REVIEW_LIMIT: usize = 100;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            "copyright" | "series" => self.copyright,
            "artist" | "creator" => self.artist,
            "species" => self.species,
            "rating" => self.rating,
            _ => self.general,
        }
    }
}

/// An active tagger session wrapping an ONNX Runtime session + parsed labels.
pub struct TaggerSession {
    runtime: SessionRuntime,
    backend: String,
    labels: Vec<LabelEntry>,
    input_size: u32,
    channel_order: ChannelOrder,
    output_activation: OutputActivation,
    slug: String,
}

enum SessionRuntime {
    Ort(ort::session::Session),
    #[cfg(target_os = "macos")]
    CoreMl {
        model: coreml_native::Model,
        input_name: String,
        output_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputSpec {
    pub input_size: u32,
    pub channel_order: ChannelOrder,
}

#[derive(Clone)]
pub struct PreparedInput {
    spec: InputSpec,
    batch_size: usize,
    values: Arc<[f32]>,
}

impl TaggerSession {
    /// Create a new tagger session, loading the ONNX model and labels from disk.
    pub fn load(
        model_dir: &Path,
        slug: &str,
        input_size: u32,
        channel_order: ChannelOrder,
        output_activation: OutputActivation,
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
        let (runtime, backend) = create_runtime(model_dir, &model_path, input_size, labels.len())?;
        if let SessionRuntime::Ort(session) = &runtime {
            validate_session_contract(session, input_size, labels.len())?;
        }

        tracing::info!(slug, labels = labels.len(), "AI tagger session loaded");

        Ok(Self {
            runtime,
            backend,
            labels,
            input_size,
            channel_order,
            output_activation,
            slug: slug.to_string(),
        })
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn gpu_backend(&self) -> String {
        self.backend.clone()
    }

    pub fn input_spec(&self) -> InputSpec {
        InputSpec {
            input_size: self.input_size,
            channel_order: self.channel_order,
        }
    }

    /// Run inference and return suggestions plus the strongest near misses for review.
    pub fn predict(
        &mut self,
        image_bytes: &[u8],
        thresholds: &Thresholds,
    ) -> Result<Vec<TagPrediction>, String> {
        let started = Instant::now();
        let input = prepare_input(image_bytes, self.input_spec())?;
        let preprocess_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.predict_prepared(&input, thresholds, preprocess_ms)
    }

    pub fn predict_prepared(
        &mut self,
        input: &PreparedInput,
        thresholds: &Thresholds,
        preprocess_ms: f64,
    ) -> Result<Vec<TagPrediction>, String> {
        let mut batches = self.predict_prepared_batch(input, thresholds, preprocess_ms)?;
        batches
            .pop()
            .ok_or_else(|| "Model produced no prediction batch".to_string())
    }

    pub fn predict_prepared_batch(
        &mut self,
        input: &PreparedInput,
        thresholds: &Thresholds,
        preprocess_ms: f64,
    ) -> Result<Vec<Vec<TagPrediction>>, String> {
        if input.spec != self.input_spec() {
            return Err(format!(
                "Prepared input {:?} does not match model input {:?}",
                input.spec,
                self.input_spec()
            ));
        }
        let inference_started = Instant::now();
        let logits = match &mut self.runtime {
            SessionRuntime::Ort(session) => {
                let input_value = ort::value::TensorRef::from_array_view((
                    [
                        input.batch_size as i64,
                        i64::from(input.spec.input_size),
                        i64::from(input.spec.input_size),
                        3,
                    ],
                    Arc::clone(&input.values),
                ))
                .map_err(|e| format!("Failed to create input tensor: {e}"))?;
                let outputs = session
                    .run(ort::inputs![input_value])
                    .map_err(|e| format!("Inference failed: {e}"))?;
                if outputs.len() == 0 {
                    return Err("Model produced no output tensors".into());
                }
                let (_, logits) = outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| format!("Failed to extract output tensor: {e}"))?;
                logits.to_vec()
            }
            #[cfg(target_os = "macos")]
            SessionRuntime::CoreMl {
                model,
                input_name,
                output_name,
            } => {
                let values_per_image = input.values.len() / input.batch_size;
                let mut logits = Vec::with_capacity(self.labels.len() * input.batch_size);
                for values in input.values.chunks_exact(values_per_image) {
                    let tensor = coreml_native::BorrowedTensor::from_f32(
                        values,
                        &[
                            1,
                            input.spec.input_size as usize,
                            input.spec.input_size as usize,
                            3,
                        ],
                    )
                    .map_err(|error| format!("Failed to create Core ML input: {error}"))?;
                    let prediction = model
                        .predict(&[(input_name, &tensor)])
                        .map_err(|error| format!("Core ML inference failed: {error}"))?;
                    let (output, _) = prediction
                        .get_f32(output_name)
                        .map_err(|error| format!("Failed to read Core ML output: {error}"))?;
                    logits.extend(output);
                }
                logits
            }
        };
        let inference_ms = inference_started.elapsed().as_secs_f64() * 1000.0;

        let expected_outputs = self.labels.len() * input.batch_size;
        if logits.len() != expected_outputs {
            return Err(format!(
                "Model output count {} does not match {} labels across {} images",
                logits.len(),
                self.labels.len(),
                input.batch_size
            ));
        }

        let postprocess_started = Instant::now();
        let predictions = logits
            .chunks_exact(self.labels.len())
            .map(|batch| self.rank_predictions(batch, thresholds))
            .collect::<Result<Vec<_>, _>>()?;
        let postprocess_ms = postprocess_started.elapsed().as_secs_f64() * 1000.0;

        tracing::debug!(
            target: "ai_inference",
            model = self.slug,
            backend = self.backend,
            preprocess_ms,
            inference_ms,
            postprocess_ms,
            batch_size = input.batch_size,
            predictions = predictions.iter().map(Vec::len).sum::<usize>(),
            "AI inference completed"
        );

        Ok(predictions)
    }

    fn rank_predictions(
        &self,
        logits: &[f32],
        thresholds: &Thresholds,
    ) -> Result<Vec<TagPrediction>, String> {
        let mut ranked = Vec::with_capacity(logits.len());
        for (index, &logit) in logits.iter().enumerate() {
            ranked.push((index, interpret_output(logit, self.output_activation)?));
        }
        ranked.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut below_threshold = 0;
        Ok(ranked
            .into_iter()
            .filter_map(|(index, confidence)| {
                let label = &self.labels[index];
                if confidence < thresholds.for_namespace(&label.namespace) {
                    if below_threshold >= BELOW_THRESHOLD_REVIEW_LIMIT {
                        return None;
                    }
                    below_threshold += 1;
                }
                Some(TagPrediction {
                    tag: label.name.clone(),
                    namespace: label.namespace.clone(),
                    confidence,
                    model: self.slug.clone(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
fn retain_review_predictions(predictions: &mut Vec<TagPrediction>, thresholds: &Thresholds) {
    let mut below_threshold = 0;
    predictions.retain(|prediction| {
        if prediction.confidence >= thresholds.for_namespace(&prediction.namespace) {
            return true;
        }
        if below_threshold >= BELOW_THRESHOLD_REVIEW_LIMIT {
            return false;
        }
        below_threshold += 1;
        true
    });
}

fn create_runtime(
    model_dir: &Path,
    model_path: &Path,
    input_size: u32,
    label_count: usize,
) -> Result<(SessionRuntime, String), String> {
    tracing::info!(model = %model_dir.display(), "Loading AI model runtime");

    #[cfg(target_os = "macos")]
    if let Some(runtime) = load_native_coreml(model_dir, input_size, label_count)? {
        return Ok((runtime, "Core ML GPU/ANE".into()));
    }

    #[cfg(target_os = "windows")]
    {
        use ort::ep::DirectML;

        let provider = DirectML::default().build().error_on_failure();
        let accelerated = (|| -> ort::Result<ort::session::Session> {
            let mut builder = ort::session::Session::builder()?
                .with_parallel_execution(false)?
                .with_memory_pattern(false)?
                .with_execution_providers([provider])?;
            builder.commit_from_file(model_path)
        })();
        match accelerated {
            Ok(session) => return Ok((SessionRuntime::Ort(session), "DirectML GPU".into())),
            Err(error) => tracing::warn!(%error, "DirectML session failed; falling back to CPU"),
        }
    }

    #[cfg(target_os = "linux")]
    {
        use ort::ep::WebGPU;

        let provider = WebGPU::default()
            .with_enable_graph_capture(true)
            .build()
            .error_on_failure();
        let accelerated = (|| -> ort::Result<ort::session::Session> {
            let mut builder =
                ort::session::Session::builder()?.with_execution_providers([provider])?;
            builder.commit_from_file(model_path)
        })();
        match accelerated {
            Ok(session) => return Ok((SessionRuntime::Ort(session), "WebGPU GPU".into())),
            Err(error) => tracing::warn!(%error, "WebGPU session failed; falling back to CPU"),
        }
    }

    ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(model_path))
        .map(|session| (SessionRuntime::Ort(session), "CPU".into()))
        .map_err(|e| format!("Failed to load ONNX model: {e}"))
}

#[cfg(target_os = "macos")]
fn load_native_coreml(
    model_dir: &Path,
    input_size: u32,
    label_count: usize,
) -> Result<Option<SessionRuntime>, String> {
    let package = model_dir.join("model.mlpackage");
    let compiled = model_dir.join("model.mlmodelc");
    if !compiled.exists() {
        if !package.exists() {
            return Ok(None);
        }
        let temporary = coreml_native::compile_model(&package)
            .map_err(|error| format!("Failed to compile Core ML model: {error}"))?;
        std::fs::rename(&temporary, &compiled).map_err(|error| {
            format!(
                "Failed to preserve compiled Core ML model {}: {error}",
                compiled.display()
            )
        })?;
    }
    let model = coreml_native::Model::load(&compiled, coreml_native::ComputeUnits::All)
        .map_err(|error| format!("Failed to load native Core ML model: {error}"))?;
    let inputs = model.inputs();
    let expected_input = [1, input_size as usize, input_size as usize, 3];
    if inputs.len() != 1 || inputs[0].shape() != Some(&expected_input[..]) {
        return Err(format!(
            "Core ML model must have one float input with shape {expected_input:?}; found {inputs:?}"
        ));
    }
    let outputs = model.outputs();
    if outputs.len() != 1 || outputs[0].shape() != Some(&[1, label_count][..]) {
        return Err(format!(
            "Core ML model must have one [1,{label_count}] output; found {outputs:?}"
        ));
    }
    Ok(Some(SessionRuntime::CoreMl {
        input_name: inputs[0].name().to_string(),
        output_name: outputs[0].name().to_string(),
        model,
    }))
}

fn validate_session_contract(
    session: &ort::session::Session,
    input_size: u32,
    label_count: usize,
) -> Result<(), String> {
    use ort::value::{TensorElementType, ValueType};

    if session.inputs().len() != 1 || session.outputs().len() != 1 {
        return Err(format!(
            "Tagger model must have one input and one output; found {} inputs and {} outputs",
            session.inputs().len(),
            session.outputs().len()
        ));
    }
    let expected_size = i64::from(input_size);
    match session.inputs()[0].dtype() {
        ValueType::Tensor { ty, shape, .. }
            if *ty == TensorElementType::Float32
                && shape.len() == 4
                && matches!(shape[0], -1 | 1)
                && shape[1] == expected_size
                && shape[2] == expected_size
                && shape[3] == 3 => {}
        other => {
            return Err(format!(
                "Tagger model input must be float32 NHWC [1,{input_size},{input_size},3], found {other}"
            ))
        }
    }
    match session.outputs()[0].dtype() {
        ValueType::Tensor { ty, shape, .. }
            if *ty == TensorElementType::Float32
                && shape.len() == 2
                && matches!(shape[0], -1 | 1)
                && shape[1] == label_count as i64 => {}
        other => {
            return Err(format!(
                "Tagger model output must be float32 [1,{label_count}], found {other}"
            ))
        }
    }
    Ok(())
}

/// Preprocess raw image bytes into a float32 tensor of shape [1, H, W, 3] (NHWC).
///
/// Steps:
/// 1. Decode image
/// 2. Pad to square with white background (preserving aspect ratio)
/// 3. Resize to model input size
/// 4. Convert byte channels directly to float values in `[0, 255]`
/// 5. Reorder channels if model expects BGR
pub fn prepare_input(image_bytes: &[u8], spec: InputSpec) -> Result<PreparedInput, String> {
    prepare_inputs(&[image_bytes], spec)
}

pub fn prepare_inputs(image_bytes: &[&[u8]], spec: InputSpec) -> Result<PreparedInput, String> {
    if image_bytes.is_empty() {
        return Err("At least one image is required for AI preprocessing".into());
    }
    let image_values = (spec.input_size * spec.input_size * 3) as usize;
    let mut values = Vec::with_capacity(image_values * image_bytes.len());
    for bytes in image_bytes {
        append_preprocessed_image(bytes, spec, &mut values)?;
    }
    Ok(PreparedInput {
        spec,
        batch_size: image_bytes.len(),
        values: Arc::from(values),
    })
}

fn append_preprocessed_image(
    image_bytes: &[u8],
    spec: InputSpec,
    values: &mut Vec<f32>,
) -> Result<(), String> {
    let img =
        image::load_from_memory(image_bytes).map_err(|e| format!("Failed to decode image: {e}"))?;

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let max_dim = w.max(h).max(1);
    let resized_width = ((u64::from(w) * u64::from(spec.input_size) + u64::from(max_dim) / 2)
        / u64::from(max_dim))
    .max(1) as u32;
    let resized_height = ((u64::from(h) * u64::from(spec.input_size) + u64::from(max_dim) / 2)
        / u64::from(max_dim))
    .max(1) as u32;
    let resized = image::imageops::resize(
        &rgb,
        resized_width,
        resized_height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut padded = image::RgbImage::from_pixel(
        spec.input_size,
        spec.input_size,
        image::Rgb([255, 255, 255]),
    );
    image::imageops::overlay(
        &mut padded,
        &resized,
        i64::from((spec.input_size - resized_width) / 2),
        i64::from((spec.input_size - resized_height) / 2),
    );

    for pixel in padded.pixels() {
        let channels = match spec.channel_order {
            ChannelOrder::Rgb => [pixel[0], pixel[1], pixel[2]],
            ChannelOrder::Bgr => [pixel[2], pixel[1], pixel[0]],
        };
        values.extend(channels.into_iter().map(f32::from));
    }
    Ok(())
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn interpret_output(value: f32, activation: OutputActivation) -> Result<f32, String> {
    let confidence = match activation {
        OutputActivation::Probability => value,
        OutputActivation::Logit => sigmoid(value),
    };
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Err(format!(
            "Model produced invalid confidence value {confidence}"
        ));
    }
    Ok(confidence)
}

/// Map of model slug → loaded TaggerSession, behind an async mutex.
pub type SharedTaggerSessions = Arc<
    tokio::sync::Mutex<std::collections::HashMap<String, Arc<std::sync::Mutex<TaggerSession>>>>,
>;

pub type SharedPredictionCache =
    Arc<std::sync::Mutex<lru::LruCache<String, Arc<Vec<TagPrediction>>>>>;

/// Create a new empty sessions map.
pub fn new_shared_sessions() -> SharedTaggerSessions {
    Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn new_prediction_cache() -> SharedPredictionCache {
    Arc::new(std::sync::Mutex::new(lru::LruCache::new(
        NonZeroUsize::new(512).expect("prediction cache capacity is non-zero"),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    fn png(pixel: Rgb<u8>) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, pixel));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn preprocessing_is_nhwc_raw_bgr() {
        let input = prepare_input(
            &png(Rgb([10, 20, 30])),
            InputSpec {
                input_size: 1,
                channel_order: ChannelOrder::Bgr,
            },
        )
        .unwrap();
        assert_eq!(&*input.values, &[30.0, 20.0, 10.0]);
    }

    #[test]
    fn batch_preprocessing_reuses_one_contiguous_model_input() {
        let first = png(Rgb([10, 20, 30]));
        let second = png(Rgb([40, 50, 60]));
        let input = prepare_inputs(
            &[&first, &second],
            InputSpec {
                input_size: 1,
                channel_order: ChannelOrder::Bgr,
            },
        )
        .unwrap();

        assert_eq!(input.batch_size, 2);
        assert_eq!(&*input.values, &[30.0, 20.0, 10.0, 60.0, 50.0, 40.0]);
    }

    #[test]
    fn output_activation_is_explicit_and_validated() {
        assert_eq!(
            interpret_output(0.25, OutputActivation::Probability).unwrap(),
            0.25
        );
        assert_eq!(interpret_output(0.0, OutputActivation::Logit).unwrap(), 0.5);
        assert!(interpret_output(1.1, OutputActivation::Probability).is_err());
        assert!(interpret_output(f32::NAN, OutputActivation::Probability).is_err());
    }

    #[test]
    fn review_predictions_include_strongest_results_below_threshold() {
        let thresholds = Thresholds {
            general: 0.8,
            character: 0.8,
            copyright: 0.8,
            artist: 0.8,
            species: 0.8,
            rating: 0.8,
        };
        let mut predictions = (0..180)
            .map(|index| TagPrediction {
                tag: format!("tag-{index}"),
                namespace: "general".into(),
                confidence: 1.0 - index as f32 / 200.0,
                model: "test".into(),
            })
            .collect::<Vec<_>>();

        retain_review_predictions(&mut predictions, &thresholds);

        assert_eq!(predictions.len(), 100 + 41);
        assert!(predictions
            .iter()
            .any(|prediction| prediction.confidence < 0.8));
        assert_eq!(
            predictions.last().map(|prediction| prediction.tag.as_str()),
            Some("tag-140")
        );
    }
}
