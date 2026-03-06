# PBI-255: OLE container detection and type disambiguation

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `/Users/midona/Code/imaginator/core/src/files/mod.rs` currently maps `UndeterminedOle` directly to `ApplicationDoc`.
2. The code contains `TODO: Port OLE file inspection`, which means legacy Office formats are not disambiguated.

## Problem
Legacy OLE container files (`.doc`, `.xls`, `.ppt`, etc.) are all treated as one MIME type (`ApplicationDoc`). This can cause incorrect metadata handling, preview routing, and UX labels for non-Word OLE files.

## Scope
- `/Users/midona/Code/imaginator/core/src/files/mod.rs`
- `/Users/midona/Code/imaginator/core/src/files/mime.rs`
- `/Users/midona/Code/imaginator/core/src/files/office.rs`

## Implementation
1. Add OLE stream/prog-id inspection to distinguish Word/Excel/PowerPoint legacy formats.
2. Map detected types to correct internal MIME variants.
3. Keep current fallback to `ApplicationDoc` when disambiguation fails.
4. Add instrumentation/logging for unknown OLE signatures.

## Acceptance Criteria
1. Legacy Word/Excel/PowerPoint files are assigned correct MIME variants.
2. Unknown OLE containers fall back safely without import failure.
3. No regression for existing Office OOXML detection paths.

## Test Cases
1. Import `.doc` (OLE) → detected as Word legacy type.
2. Import `.xls` (OLE) → detected as Excel legacy type.
3. Import `.ppt` (OLE) → detected as PowerPoint legacy type.
4. Import malformed/unknown OLE → safe fallback and no crash.

## Risk
Medium. Binary container parsing can be brittle across edge-case files; fallback path must remain robust.
