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
use super::models::{ChannelOrder, ModelAdapter, OutputActivation};

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
    adapter: ModelAdapter,
    output_activation: OutputActivation,
    slug: String,
}

enum SessionRuntime {
    Ort(ort::session::Session),
    #[cfg(target_os = "macos")]
    CoreMl {
        model: coreml_native::Model,
        input_name: String,
        padding_mask_name: Option<String>,
        output_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputSpec {
    pub input_size: u32,
    pub channel_order: ChannelOrder,
    pub adapter: ModelAdapter,
}

#[derive(Clone)]
pub struct PreparedInput {
    spec: InputSpec,
    batch_size: usize,
    values: Arc<[f32]>,
    padding_mask: Option<Arc<[bool]>>,
}

impl TaggerSession {
    /// Create a new tagger session, loading the ONNX model and labels from disk.
    pub fn load(
        model_dir: &Path,
        slug: &str,
        input_size: u32,
        channel_order: ChannelOrder,
        output_activation: OutputActivation,
        adapter: ModelAdapter,
    ) -> Result<Self, String> {
        let model_path = model_dir.join("model.onnx");
        if !model_path.exists() {
            return Err(format!("Model file not found: {}", model_path.display()));
        }

        let labels = super::labels::parse_model_labels(model_dir)?;
        let coreml_current = super::models::find_model(slug)
            .is_some_and(|model| super::models::coreml_artifact_is_current(model_dir, &model));
        let (runtime, backend) = create_runtime(
            model_dir,
            &model_path,
            input_size,
            labels.len(),
            adapter,
            coreml_current,
        )?;
        match &runtime {
            SessionRuntime::Ort(session) => {
                validate_session_contract(session, input_size, labels.len(), adapter)?
            }
            #[cfg(target_os = "macos")]
            SessionRuntime::CoreMl { .. } => {}
        }

        tracing::info!(slug, labels = labels.len(), "AI tagger session loaded");

        Ok(Self {
            runtime,
            backend,
            labels,
            input_size,
            channel_order,
            adapter,
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
            adapter: self.adapter,
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
        let native_probabilities = runtime_outputs_probabilities(&self.runtime);
        let logits = match &mut self.runtime {
            SessionRuntime::Ort(session) => run_ort(session, input)?,
            #[cfg(target_os = "macos")]
            SessionRuntime::CoreMl {
                model,
                input_name,
                padding_mask_name,
                output_name,
            } => {
                let values_per_image = input.values.len() / input.batch_size;
                let mut logits = Vec::with_capacity(self.labels.len() * input.batch_size);
                for (index, values) in input.values.chunks_exact(values_per_image).enumerate() {
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
                    let prediction = if let (Some(mask), Some(padding_mask_name)) =
                        (&input.padding_mask, padding_mask_name.as_ref())
                    {
                        let mask_values = mask[index * input.spec.input_size.pow(2) as usize
                            ..(index + 1) * input.spec.input_size.pow(2) as usize]
                            .iter()
                            .map(|value| f32::from(*value))
                            .collect::<Vec<_>>();
                        let mask_tensor = coreml_native::BorrowedTensor::from_f32(
                            &mask_values,
                            &[
                                1,
                                input.spec.input_size as usize,
                                input.spec.input_size as usize,
                            ],
                        )
                        .map_err(|error| format!("Failed to create Core ML mask: {error}"))?;
                        model.predict(&[(input_name, &tensor), (padding_mask_name, &mask_tensor)])
                    } else {
                        model.predict(&[(input_name, &tensor)])
                    }
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
            .map(|batch| {
                self.rank_predictions(
                    batch,
                    thresholds,
                    if native_probabilities {
                        OutputActivation::Probability
                    } else {
                        self.output_activation
                    },
                )
            })
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
        activation: OutputActivation,
    ) -> Result<Vec<TagPrediction>, String> {
        let mut ranked = Vec::with_capacity(logits.len());
        for (index, &logit) in logits.iter().enumerate() {
            ranked.push((index, interpret_output(logit, activation)?));
        }
        ranked.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut below_threshold = 0;
        Ok(ranked
            .into_iter()
            .filter_map(|(index, confidence)| {
                let label = &self.labels[index];
                if label.name.starts_with('<') && label.name.ends_with('>') {
                    return None;
                }
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

fn runtime_outputs_probabilities(runtime: &SessionRuntime) -> bool {
    #[cfg(target_os = "macos")]
    return matches!(runtime, SessionRuntime::CoreMl { .. });
    #[cfg(not(target_os = "macos"))]
    {
        let _ = runtime;
        false
    }
}

fn run_ort(session: &mut ort::session::Session, input: &PreparedInput) -> Result<Vec<f32>, String> {
    let size = input.spec.input_size as usize;
    let outputs = match input.spec.adapter {
        ModelAdapter::Wd => {
            let tensor = ort::value::TensorRef::from_array_view((
                [input.batch_size as i64, size as i64, size as i64, 3],
                Arc::clone(&input.values),
            ))
            .map_err(|e| format!("Failed to create model input: {e}"))?;
            session.run(ort::inputs![tensor])
        }
        ModelAdapter::OppaiOracle => {
            let values = normalized_nchw(input, [0.5; 3], [0.5; 3]);
            let image = ort::value::Tensor::from_array((
                [input.batch_size, 3, size, size],
                values.into_boxed_slice(),
            ))
            .map_err(|e| format!("Failed to create model input: {e}"))?;
            let mask = ndarray::Array3::from_shape_vec(
                (input.batch_size, size, size),
                input
                    .padding_mask
                    .as_ref()
                    .ok_or_else(|| "OppaiOracle input is missing its padding mask".to_string())?
                    .to_vec(),
            )
            .map_err(|e| format!("Failed to create padding mask: {e}"))?;
            let mask = ort::value::Tensor::from_array(mask)
                .map_err(|e| format!("Failed to create padding mask: {e}"))?;
            session.run(ort::inputs![image, mask])
        }
    }
    .map_err(|e| format!("Inference failed: {e}"))?;
    if outputs.len() == 0 {
        return Err("Model produced no output tensors".into());
    }
    let (_, logits) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("Failed to extract output tensor: {e}"))?;
    Ok(logits.to_vec())
}

fn normalized_nchw(input: &PreparedInput, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    let pixels = input.spec.input_size.pow(2) as usize;
    let mut output = Vec::with_capacity(input.values.len());
    for image in input.values.chunks_exact(pixels * 3) {
        for channel in 0..3 {
            output.extend(
                image
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|pixel| (pixel[channel] / 255.0 - mean[channel]) / std[channel]),
            );
        }
    }
    output
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
    adapter: ModelAdapter,
    coreml_current: bool,
) -> Result<(SessionRuntime, String), String> {
    tracing::info!(model = %model_dir.display(), "Loading AI model runtime");

    #[cfg(target_os = "macos")]
    if coreml_current {
        match load_native_coreml(model_dir, input_size, label_count, adapter) {
            Ok(Some(runtime)) => return Ok((runtime, "Core ML GPU/ANE".into())),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "Core ML model is incompatible; falling back to ONNX")
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (input_size, label_count, adapter, coreml_current);

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
    adapter: ModelAdapter,
) -> Result<Option<SessionRuntime>, String> {
    let compiled = model_dir.join("model.mlmodelc");
    if !compiled.exists() {
        return Ok(None);
    }
    let model = coreml_native::Model::load(&compiled, coreml_native::ComputeUnits::All)
        .map_err(|error| format!("Failed to load native Core ML model: {error}"))?;
    let inputs = model.inputs();
    let expected_input = [1, input_size as usize, input_size as usize, 3];
    let expected_inputs = if adapter == ModelAdapter::OppaiOracle {
        2
    } else {
        1
    };
    if inputs.len() != expected_inputs || inputs[0].shape() != Some(&expected_input[..]) {
        return Err(format!(
            "Core ML model must have {expected_inputs} input(s) beginning with shape {expected_input:?}; found {inputs:?}"
        ));
    }
    if adapter == ModelAdapter::OppaiOracle
        && inputs[1].shape() != Some(&[1, input_size as usize, input_size as usize][..])
    {
        return Err(format!(
            "OppaiOracle Core ML padding mask has the wrong shape: {:?}",
            inputs[1]
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
        padding_mask_name: (adapter == ModelAdapter::OppaiOracle)
            .then(|| inputs[1].name().to_string()),
        output_name: outputs[0].name().to_string(),
        model,
    }))
}

fn validate_session_contract(
    session: &ort::session::Session,
    input_size: u32,
    label_count: usize,
    adapter: ModelAdapter,
) -> Result<(), String> {
    use ort::value::{TensorElementType, ValueType};

    let expected_inputs = if adapter == ModelAdapter::OppaiOracle {
        2
    } else {
        1
    };
    if session.inputs().len() != expected_inputs || session.outputs().len() != 1 {
        return Err(format!(
            "Tagger model must have {expected_inputs} input(s) and one output; found {} inputs and {} outputs",
            session.inputs().len(),
            session.outputs().len()
        ));
    }
    let expected_size = i64::from(input_size);
    let channel_first = adapter != ModelAdapter::Wd;
    match session.inputs()[0].dtype() {
        ValueType::Tensor { ty, shape, .. }
            if *ty == TensorElementType::Float32
                && shape.len() == 4
                && matches!(shape[0], -1 | 1)
                && if channel_first {
                    shape[1] == 3 && shape[2] == expected_size && shape[3] == expected_size
                } else {
                    shape[1] == expected_size && shape[2] == expected_size && shape[3] == 3
                } => {}
        other => {
            return Err(format!(
                "Tagger model input has the wrong tensor contract for {adapter:?}: {other}"
            ))
        }
    }
    if adapter == ModelAdapter::OppaiOracle {
        match session.inputs()[1].dtype() {
            ValueType::Tensor { ty, shape, .. }
                if *ty == TensorElementType::Bool
                    && shape.len() == 3
                    && matches!(shape[0], -1 | 1)
                    && shape[1] == expected_size
                    && shape[2] == expected_size => {}
            other => {
                return Err(format!(
                    "OppaiOracle padding mask must be bool [1,{input_size},{input_size}], found {other}"
                ))
            }
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
/// 2. Letterbox using the model's registered background (preserving aspect ratio)
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
    let mut padding_mask = (spec.adapter == ModelAdapter::OppaiOracle)
        .then(|| Vec::with_capacity((image_values / 3) * image_bytes.len()));
    for bytes in image_bytes {
        append_preprocessed_image(bytes, spec, &mut values, padding_mask.as_mut())?;
    }
    Ok(PreparedInput {
        spec,
        batch_size: image_bytes.len(),
        values: Arc::from(values),
        padding_mask: padding_mask.map(Arc::from),
    })
}

fn append_preprocessed_image(
    image_bytes: &[u8],
    spec: InputSpec,
    values: &mut Vec<f32>,
    mut padding_mask: Option<&mut Vec<bool>>,
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
    let background = match spec.adapter {
        ModelAdapter::Wd => 255,
        ModelAdapter::OppaiOracle => 114,
    };
    let mut padded = image::RgbImage::from_pixel(
        spec.input_size,
        spec.input_size,
        image::Rgb([background; 3]),
    );
    let offset_x = (spec.input_size - resized_width) / 2;
    let offset_y = (spec.input_size - resized_height) / 2;
    image::imageops::overlay(
        &mut padded,
        &resized,
        i64::from(offset_x),
        i64::from(offset_y),
    );

    if let Some(mask) = padding_mask.as_mut() {
        for y in 0..spec.input_size {
            for x in 0..spec.input_size {
                mask.push(
                    x < offset_x
                        || x >= offset_x + resized_width
                        || y < offset_y
                        || y >= offset_y + resized_height,
                );
            }
        }
    }

    for pixel in padded.pixels() {
        let channels = match spec.channel_order {
            ChannelOrder::Rgb => [pixel[0], pixel[1], pixel[2]],
            ChannelOrder::Bgr => [pixel[2], pixel[1], pixel[0]],
        };
        values.extend(channels.into_iter().map(f32::from));
    }
    Ok(())
}

fn interpret_output(value: f32, activation: OutputActivation) -> Result<f32, String> {
    let confidence = match activation {
        OutputActivation::Probability => value,
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
        png_with_size(1, 1, pixel)
    }

    fn png_with_size(width: u32, height: u32, pixel: Rgb<u8>) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, pixel));
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
                adapter: ModelAdapter::Wd,
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
                adapter: ModelAdapter::Wd,
            },
        )
        .unwrap();

        assert_eq!(input.batch_size, 2);
        assert_eq!(&*input.values, &[30.0, 20.0, 10.0, 60.0, 50.0, 40.0]);
    }

    #[test]
    fn oppai_preprocessing_uses_gray_letterbox_and_padding_mask() {
        let input = prepare_input(
            &png_with_size(1, 2, Rgb([10, 20, 30])),
            InputSpec {
                input_size: 2,
                channel_order: ChannelOrder::Rgb,
                adapter: ModelAdapter::OppaiOracle,
            },
        )
        .unwrap();

        assert_eq!(
            input.padding_mask.as_deref(),
            Some(&[false, true, false, true][..])
        );
        assert_eq!(
            &input.values[0..6],
            &[10.0, 20.0, 30.0, 114.0, 114.0, 114.0]
        );
    }

    #[test]
    fn output_activation_is_explicit_and_validated() {
        assert_eq!(
            interpret_output(0.25, OutputActivation::Probability).unwrap(),
            0.25
        );
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
