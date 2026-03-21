# PBI-313: Media processing adapter registry and pipeline breakup

## Priority
P2

## Audit Status (2026-03-15)
Status: **Implemented**

Evidence:
1. The current media-processing entrypoint is `core/src/media_processing/mod.rs`, and it now acts as facade/orchestration glue instead of a 1000-line mixed-responsibility file.
2. Core pipeline stages are split into `core/src/media_processing/detection.rs`, `core/src/media_processing/analysis.rs`, `core/src/media_processing/hashing.rs`, and `core/src/media_processing/thumbnail.rs`.
3. Format routing is isolated in `core/src/media_processing/adapters.rs`, so archive/document/specialty handling is no longer smeared across the facade.
4. Existing public API remains stable while adding a new format is now localized to the adapter registry plus the format-specific module.

## Problem
Media processing is now effectively a format capability platform, but it is still organized as a large utility module with helper submodules. That makes ownership of format support, analysis capabilities, and thumbnail/metadata extraction harder to extend cleanly.

## Scope
- `core/src/media_processing/mod.rs`
- `core/src/media_processing/archive.rs`
- `core/src/media_processing/ffmpeg.rs`
- `core/src/media_processing/office.rs`
- `core/src/media_processing/pdf.rs`
- `core/src/media_processing/specialty.rs`
- `core/src/media_processing/svg.rs`
- related imports after `PBI-237`

## Implementation
1. After `PBI-237`, introduce a media-processing adapter/capability registry.
2. Separate core pipeline stages:
   - detect
   - inspect
   - hash
   - thumbnail/render
   - extract metadata
3. Make format-specific adapters implement those capabilities explicitly.
4. Reduce the amount of branching and format routing concentrated in `mod.rs`.

## Acceptance Criteria
1. Media processing has explicit adapter boundaries.
2. `mod.rs` becomes orchestration glue rather than a giant capability file.
3. Adding a new media format is more localized.
4. Existing detection/thumbnail/metadata behavior remains unchanged.

## Test Cases
1. Existing image/video/document imports still work.
2. Thumbnail generation still succeeds across supported formats.
3. MIME detection and analysis results remain stable.

## Risk
Medium. Broad refactor, but lower immediate product risk than runtime/state work.
