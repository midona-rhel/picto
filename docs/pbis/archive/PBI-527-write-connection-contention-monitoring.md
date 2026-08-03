# PBI-527: Write connection contention monitoring

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The concern is based on architectural analysis of the single-writer pattern. For a desktop app with one user, write contention is unlikely to be a problem today. This PBI is about adding observability so that contention becomes visible if it occurs — not about changing the architecture.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: All writes serialize through a single `Arc<Mutex<Connection>>` in `core/src/sqlite/diagnostics.rs`. The `with_conn()` and `with_conn_mut()` methods acquire the write lock via `spawn_blocking`. There is no metric for how long callers wait to acquire the write lock (contention time), only how long the query itself takes (execution time). The slow query logger (`SLOW_WRITE_WARN_MS = 100ms`) measures only execution, not wait time.

## Problem
The Rust core uses a single write connection protected by `Mutex<Connection>`. This is correct for SQLite's single-writer model. However, there is no visibility into write lock contention — how long concurrent writers wait before acquiring the lock.

Under concurrent load (subscription sync + manual import + AI tagging), write operations queue behind the mutex. The current slow-query diagnostics only measure query execution time, not the total latency including wait time. A query that runs in 5ms but waits 500ms for the lock appears fast in diagnostics.

## Scope
- `core/src/sqlite/diagnostics.rs` — `with_conn()` and `with_conn_mut()` methods
- `core/src/perf.rs` — performance snapshot (if exists)

## Implementation
1. **Measure lock acquisition time**: In `with_conn()` and `with_conn_mut()`, capture `Instant::now()` before acquiring the mutex and after. The delta is contention time.
2. **Log contention warnings**: If contention exceeds a threshold (e.g., 50ms), log at `warn!` level with the labeled query kind and contention duration.
3. **Expose contention metrics**: Add `write_contention_max_ms` and `write_contention_count` to the perf snapshot returned by `get_perf_snapshot` / `check_perf_slo`.
4. **No architectural changes**: This PBI is purely observability — the single-writer pattern remains unchanged.

## Acceptance Criteria
1. Write lock contention time is measured separately from query execution time.
2. Contention exceeding 50ms triggers a `warn!` log with context.
3. `get_perf_snapshot` includes contention metrics.
4. No performance regression from the measurement itself (Instant::now is ~25ns).

## Test Cases
1. Single-threaded write operations → contention time is near zero.
2. Simulated concurrent writes (two tasks writing simultaneously) → contention time is measurable and logged.
3. `get_perf_snapshot` returns contention metrics after write operations.

## Risk
Low. Adding timing instrumentation to the lock acquisition path is non-invasive. The only risk is if `Instant::now()` has unexpected overhead, but it's a monotonic clock read (~25ns on all platforms).
