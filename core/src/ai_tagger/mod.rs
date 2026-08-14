//! AI auto-tagging via ONNX inference (WD14 / E621 tagger models).
//!
//! Provides atomic model management, CPU inference, and label parsing
//! for SmilingWolf's WDv3 / E621 tagger models.

pub mod download;
pub mod inference;
pub mod labels;
pub mod models;
