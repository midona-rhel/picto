# PBI-309: Media processing adapter registry and pipeline breakup

## Priority
P2

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/files/mod.rs` is 1000+ lines and mixes MIME detection, hashing, thumbnail generation, file analysis, and format routing.
2. Format-specific modules (`office.rs`, `pdf.rs`, `specialty.rs`, `svg.rs`, `ffmpeg.rs`) behave like helpers rather than clearly defined adapters.
3. The module is already important enough that `PBI-237` exists just to rename the folder away from the overloaded `files/` name.
4. As more formats are added, the current helper-centric structure will keep growing in one place.

## Problem
Media processing is now effectively a format capability platform, but it is still organized as a large utility module with helper submodules. That makes ownership of format support, analysis capabilities, and thumbnail/metadata extraction harder to extend cleanly.

## Scope
- `core/src/files/mod.rs`
- `core/src/files/archive.rs`
- `core/src/files/ffmpeg.rs`
- `core/src/files/image_metadata.rs`
- `core/src/files/office.rs`
- `core/src/files/pdf.rs`
- `core/src/files/specialty.rs`
- `core/src/files/svg.rs`
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
