# PBI-256: OLE metadata extraction (word count and core properties)

## Priority
P3

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `/Users/midona/Code/imaginator/core/src/files/office.rs` has `ole_document_word_count()` returning `Ok(None)` with a TODO.
2. Current implementation explicitly states full OLE metadata parsing is unavailable.

## Problem
For legacy OLE Office files, metadata fields such as word count are unavailable, reducing metadata parity with other file formats and with prior hydrus expectations.

## Scope
- `/Users/midona/Code/imaginator/core/src/files/office.rs`
- Optional crate integration for OLE metadata parsing

## Implementation
1. Integrate an OLE-capable parser crate (or equivalent internal parser).
2. Implement safe extraction for:
   - word count (where available)
   - selected core metadata fields when feasible.
3. Preserve graceful `None` behavior when metadata cannot be extracted.
4. Add parser-failure telemetry to avoid silent failures.

## Acceptance Criteria
1. `ole_document_word_count()` returns actual counts for supported OLE docs.
2. Unsupported/malformed files still return safe fallback without panic.
3. Extraction path has test coverage for success and failure modes.

## Test Cases
1. Known `.doc` with expected word count → returned count matches expected range.
2. Non-OLE file passed to OLE function → returns expected error path.
3. Corrupt OLE file → no panic; fallback path used.

## Risk
Medium-low. Optional metadata extraction should never block import; failure must be non-fatal.
