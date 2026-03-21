# PBI-535: Slow query observability — suppression window gap

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The observability gap was identified by reading the slow query logging implementation. If slow queries are rare in practice, this refinement may have low impact. If slow queries are common (e.g., during compiler runs on large libraries), the suppression could be masking a systemic performance issue.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `core/src/sqlite/diagnostics.rs` implements a sliding window (`SLOW_QUERY_LOG_WINDOW = 1 second`) that suppresses repeated slow queries of the same kind. Within the window, only the first occurrence is logged; subsequent occurrences increment a counter. When the window expires, a summary log with `suppressed = N` is emitted. However, if the same query is consistently slow (e.g., every sidebar refresh), the pattern manifests as one warning per second with no aggregate view — it's hard to distinguish "occasionally slow" from "always slow."

## Problem
The slow query suppression window prevents log flooding but creates an observability gap:

1. **No aggregate metric**: There's no persistent counter of total slow queries by kind. The suppression summary is a one-off log line that's hard to correlate over time.
2. **"Always slow" is indistinguishable from "sometimes slow"**: A query that exceeds 100ms on every invocation looks the same as one that exceeds 100ms once per second — both produce ~1 warning/second.
3. **No exposure via API**: The `get_perf_snapshot` / `check_perf_slo` endpoints don't include slow query counts, so the frontend and diagnostics tools have no visibility.

## Scope
- `core/src/sqlite/diagnostics.rs` — slow query tracking
- `core/src/perf.rs` — perf snapshot (if it exists)
- `core/src/dispatch/typed/system.rs` — `get_perf_snapshot` handler

## Implementation
1. **Add persistent counters**: Alongside the sliding window, maintain a `HashMap<&'static str, u64>` of total slow query counts since library open. Don't reset on window expiry.
2. **Expose via perf snapshot**: Add `slow_queries: HashMap<String, u64>` to the perf snapshot response. This lets frontend diagnostics (and future developer tools) show slow query patterns.
3. **Add rate metric**: Track `slow_queries_per_minute` (or per 10 seconds) as a sliding rate. This distinguishes "always slow" (rate = invocation rate) from "sometimes slow" (rate < invocation rate).

## Acceptance Criteria
1. `get_perf_snapshot` includes slow query counts by kind.
2. Persistent counters survive across suppression windows.
3. Existing log suppression behavior is unchanged (no log flooding).
4. A consistently slow query shows a higher rate than an occasionally slow one.

## Test Cases
1. Force a slow read query 10 times in 2 seconds → `get_perf_snapshot` shows `slow_read: 10`.
2. Force a slow read query once → `get_perf_snapshot` shows `slow_read: 1`.
3. Log output still suppressed within the 1-second window (no behavior change).
4. Two different slow query kinds → both tracked independently in the snapshot.

## Risk
Low. Adding counters to the existing tracking structure is non-invasive. The HashMap is behind the same mutex used for the sliding window, so no additional synchronization is needed.
