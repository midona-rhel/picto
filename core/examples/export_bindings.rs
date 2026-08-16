//! Explicit TypeScript binding generator for the contracts imported by the renderer.
//!
//! Run with `npm run generate:bindings`. Normal Rust tests never write source files.

use ts_rs::TS;

fn normalize_generated_bindings() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/shared/types/generated");
    let mut directories = vec![root];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).expect("read generated bindings") {
            let path = entry.expect("read generated binding entry").path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("ts") {
                let source = std::fs::read_to_string(&path).expect("read generated binding");
                let normalized = source
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                if source != normalized {
                    std::fs::write(path, normalized).expect("normalize generated binding");
                }
            }
        }
    }
}

fn main() {
    use picto_core::dispatch::typed::ai_tagger::{AiTagPredictOutput, AiTaggerStatusOutput};
    use picto_core::runtime_contract::task::RuntimeTask;

    // Export paths are relative to core/src in the existing derive attributes.
    let output_base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    AiTaggerStatusOutput::export_all_to(&output_base).unwrap();
    AiTagPredictOutput::export_all_to(&output_base).unwrap();
    RuntimeTask::export_all_to(&output_base).unwrap();

    normalize_generated_bindings();
}
