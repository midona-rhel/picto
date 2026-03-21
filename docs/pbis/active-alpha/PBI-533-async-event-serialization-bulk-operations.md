# PBI-533: Async event serialization for bulk operations

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The performance concern is based on the observation that `serde_json::to_string()` runs synchronously on the hot path for every event emission. For individual operations this is negligible. The concern applies specifically to bulk import (hundreds of events in rapid succession) where serialization cost accumulates.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: In `core/src/events.rs` line 41, `emit<T>()` calls `serde_json::to_string(payload)` synchronously before invoking the event callback. During bulk import, `ManualImportProgressEvent` is emitted for every file (hundreds or thousands of times). Each emission serializes the event struct to JSON on the calling thread.

## Problem
Event serialization in `emit<T>()` runs synchronously on the calling thread. For individual events this is fast (~1μs), but during bulk import, the cumulative cost of serializing hundreds of `ManualImportProgressEvent` structs adds overhead to the import pipeline.

The N-API callback itself is non-blocking (queued via `ThreadsafeFunction`), but the serialization happens before the queue — on the Rust worker thread that could be doing import work instead.

## Scope
- `core/src/events.rs` — `emit<T>()` function
- `core/src/import/pipeline.rs` — bulk import progress emission

## Implementation
1. **Reduce event frequency during bulk import**: Instead of emitting progress on every file, emit every N files (e.g., every 10 or every 100ms). The frontend already debounces receipt processing (50ms), so per-file granularity provides no UX benefit.
2. **Pre-serialize reusable event parts**: For `ManualImportProgressEvent`, the only changing fields are `done`, `current_file`, `imported`, `skipped`, `errors`. Consider using a pre-allocated buffer with field updates rather than full struct serialization each time.
3. **Alternative — batch emission**: Accumulate progress in the import loop and emit a single summary event at the end, with periodic progress events at fixed intervals (e.g., every 500ms).

## Acceptance Criteria
1. Bulk import of 1000 files emits ≤100 progress events (not 1000).
2. Frontend progress display still updates smoothly (at least every 500ms).
3. No change to single-file import behavior.
4. Import throughput (files/second) improves or remains unchanged.

## Test Cases
1. Import 500 files → count progress events emitted → verify ≤50.
2. Import 500 files → frontend progress bar updates at least every 500ms.
3. Import 1 file → single progress event emitted (no batching for single files).
4. Benchmark: import 1000 files before and after → verify no throughput regression.

## Risk
Low. Reducing event frequency is a simple throttle. The main risk is the frontend appearing unresponsive if the interval is too long — 500ms is a safe upper bound for perceived responsiveness.
