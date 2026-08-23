use std::path::PathBuf;

use picto_core::converter_v2::{self, ConversionMode, ConversionRequest};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = match arguments.next().and_then(|value| value.into_string().ok()).as_deref() {
        Some("--dry-run") => ConversionMode::DryRun,
        Some("--execute") => ConversionMode::Execute,
        _ => return Err(usage()),
    };
    let source_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let destination_root = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage());
    }

    let report = converter_v2::convert(ConversionRequest {
        source_root,
        destination_root,
        mode,
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("Failed to encode conversion report: {error}"))?
    );
    Ok(())
}

fn usage() -> String {
    "Usage: cargo run --example convert_library_v2 -- (--dry-run|--execute) <source.library> <destination.library>".to_string()
}
