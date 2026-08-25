use std::path::PathBuf;
use std::time::Instant;

use picto_core::ai_tagger::inference::{prepare_input, TaggerSession, Thresholds};
use picto_core::ai_tagger::models;

const WARM_UP_RUNS: usize = 5;
const MEASURED_RUNS: usize = 30;
const REFERENCE_MODEL: &str = "wd14-swinv2-v3";

fn main() -> Result<(), String> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 3 {
        return Err(
            "usage: ai_model_benchmark <models-root> <image> <model-slug> [model-slug ...]".into(),
        );
    }
    let models_root = PathBuf::from(&args[0]);
    let image_path = PathBuf::from(&args[1]);
    let image = std::fs::read(&image_path)
        .map_err(|error| format!("Failed to read {}: {error}", image_path.display()))?;
    let thresholds = Thresholds {
        general: 0.35,
        character: 0.85,
        copyright: 0.85,
        artist: 0.85,
        species: 0.35,
        rating: 0.5,
    };

    let mut results = Vec::new();
    for slug in &args[2..] {
        let model = models::find_model(slug).ok_or_else(|| format!("Unknown model '{slug}'"))?;
        let load_started = Instant::now();
        let mut session = TaggerSession::load(
            &models_root.join(slug),
            slug,
            model.input_size,
            model.channel_order,
            model.output_activation,
            model.adapter,
        )?;
        let load_ms = elapsed_ms(load_started);
        let preprocess_started = Instant::now();
        let prepared = prepare_input(&image, session.input_spec())?;
        let preprocess_ms = elapsed_ms(preprocess_started);

        for _ in 0..WARM_UP_RUNS {
            session.predict_prepared(&prepared, &thresholds, 0.0)?;
        }
        let mut samples = Vec::with_capacity(MEASURED_RUNS);
        for _ in 0..MEASURED_RUNS {
            let started = Instant::now();
            session.predict_prepared(&prepared, &thresholds, 0.0)?;
            samples.push(elapsed_ms(started));
        }
        samples.sort_by(f64::total_cmp);
        results.push(serde_json::json!({
            "slug": slug,
            "backend": session.gpu_backend(),
            "load_ms": round(load_ms),
            "preprocess_ms": round(preprocess_ms),
            "median_inference_ms": round(percentile(&samples, 0.5)),
            "p95_inference_ms": round(percentile(&samples, 0.95)),
        }));
    }

    let reference = results
        .iter()
        .find(|result| result["slug"] == REFERENCE_MODEL)
        .or_else(|| results.first())
        .and_then(|result| result["median_inference_ms"].as_f64())
        .ok_or_else(|| "Benchmark produced no reference result".to_string())?;
    for result in &mut results {
        let median = result["median_inference_ms"]
            .as_f64()
            .ok_or_else(|| "Benchmark result omitted median latency".to_string())?;
        result["relative_to_wd_swin"] = serde_json::json!(round(median / reference));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&results)
            .map_err(|error| format!("Failed to serialize benchmark results: {error}"))?
    );
    Ok(())
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index]
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
