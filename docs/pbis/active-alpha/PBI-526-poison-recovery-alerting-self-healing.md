# PBI-526: Poison recovery alerting and self-healing

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The risk assessment is based on code inspection of `core/src/poison.rs`. In practice, mutex poisoning should be rare in a well-tested codebase. The concern is about what happens when it does occur — silent recovery may mask data inconsistency. If poisoning has never been observed in production, this is a defensive hardening measure.

## Priority
P2

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `core/src/poison.rs` defines `mutex_or_recover()`, `read_or_recover()`, and `write_or_recover()` which silently recover from poisoned locks by extracting inner data and logging a `tracing::warn!`. No downstream alerting, no consistency check triggered, no state flag set. The warning is easily lost in normal log output.

## Problem
A poisoned mutex means a thread panicked while holding the lock. The data inside may be partially modified and inconsistent. The current `poison.rs` helpers recover the data and log a warning, preventing crash cascades — but they do not:

1. Surface the problem to the user or any monitoring system
2. Trigger a consistency check or self-healing rebuild
3. Track whether poisoning has occurred (no persistent flag)

If bitmap data is half-written during a compiler run and the lock poisons, the recovered bitmap store may contain incorrect membership sets. The sidebar could show wrong counts, smart folders could miss files, and the user would never know.

## Scope
- `core/src/poison.rs` — recovery helpers
- `core/src/sqlite/compilers.rs` — compiler uses bitmaps protected by locks
- `core/src/sqlite/bitmaps.rs` — BitmapStore internals (RwLock-protected)
- `core/src/events.rs` — event emission for frontend alerting

## Implementation
1. **Add atomic poison flag** in `core/src/poison.rs`:
   ```rust
   static POISON_RECOVERED: AtomicBool = AtomicBool::new(false);

   pub fn was_poison_recovered() -> bool {
       POISON_RECOVERED.load(Ordering::Relaxed)
   }
   ```
   Set this flag in all three recovery functions.

2. **Trigger RebuildAll on poison recovery**: After recovering from a poisoned lock, enqueue a `ReadModelEvent::RebuildAll` to force the compiler to recompute all derived artifacts from the source-of-truth SQLite data.

3. **Emit a diagnostic event** to the frontend: `runtime/poison_recovered` with context string. The frontend can show a non-blocking warning toast.

4. **Expose via `check_perf_slo`**: The existing `check_perf_slo` command can report `poison_recovered: true` for diagnostic purposes.

## Acceptance Criteria
1. Any poison recovery sets the `POISON_RECOVERED` flag.
2. After poison recovery, a `RebuildAll` event is enqueued within the next compiler cycle.
3. A `runtime/poison_recovered` event reaches the frontend.
4. `check_perf_slo` reports poison recovery status.
5. Existing behavior preserved: no panics, no crash cascades.

## Test Cases
1. Simulate lock poisoning in a test → verify `POISON_RECOVERED` flag is set.
2. After poison recovery → verify `RebuildAll` event is enqueued.
3. After poison recovery → verify `runtime/poison_recovered` event emitted with context string.
4. Normal operation (no poisoning) → `was_poison_recovered()` returns false.

## Risk
Low. The poison recovery mechanism already exists and works. This PBI adds observability and self-healing on top of it without changing the recovery logic itself. The RebuildAll trigger is the highest-risk addition — it must not create a feedback loop if the rebuild itself poisons.
