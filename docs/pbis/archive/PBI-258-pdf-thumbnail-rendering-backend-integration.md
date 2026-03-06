# PBI-258: PDF thumbnail rendering backend integration

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `/Users/midona/Code/imaginator/core/src/files/pdf.rs` has `generate_thumbnail_from_pdf()` returning an error for all files.
2. Code includes TODO to add a renderer (e.g., pdfium/mupdf).

## Problem
PDF files cannot generate thumbnails, resulting in degraded grid/inspector UX and non-uniform preview behavior compared to images/video.

## Scope
- `/Users/midona/Code/imaginator/core/src/files/pdf.rs`
- Thumbnail generation pipeline in import/thumbnail workers
- Build/runtime dependency handling for selected PDF renderer

## Implementation
1. Choose PDF rendering backend (pdfium or mupdf) with cross-platform packaging plan.
2. Render first page at requested thumbnail resolution.
3. Normalize output to existing thumbnail format/path conventions.
4. Add robust fallback for encrypted/corrupt PDFs.

## Acceptance Criteria
1. Imported PDFs generate thumbnails consistently on supported platforms.
2. Thumbnail generation failures do not break import.
3. Packaging/dev setup documents renderer dependency expectations.

## Test Cases
1. Standard PDF import → thumbnail present.
2. Multi-page PDF import → first page thumbnail rendered.
3. Encrypted or corrupt PDF → import continues, thumbnail failure reported gracefully.

## Risk
Medium-high. Native PDF render dependencies may complicate packaging and CI portability.
