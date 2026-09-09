# Grid preparation: local results

Date: 2026-09-05. Baseline: `d449f5ce4800c5c0876c279ca0ffaca62b3b866e`.
Tracking: https://github.com/midona-rhel/picto/issues/45. Not committed or released.

Follow-up: the cold-tail ordering bottleneck measured here is addressed in
`COLD_TAIL_RESULTS.md`. This document retains the original preparation-only measurements.

## Implemented scope

Navigation and sort preparation share the controller's bounded destination query path. History
restoration reads four canonical preview items and then the destination window, without walking
intervening pages. The workspace prewarms resident geometry during the existing transition.
The spatial index allocates bins for the occupied window rather than every pixel band from zero.
Hidden preparation does not earn thumbnail/HQ visibility dwell. First-paint readiness requires the
resident window to intersect the restored viewport. React Strict Mode remounts restart canceled
initial loading.

Navigation fades remain 170 ms out and 170 ms in. Thumbnail/HQ eligibility remains 100/250 ms,
including cached images; the existing color-to-image reveal remains. No new visual UI was added.
No native query algorithm, database schema, full-query renderer snapshot, or worker was introduced.

## Native benchmark comparison

Both phases completed all six existing benchmark jobs successfully: grid query scales through one
million; million-item window traversal and random search; general operations at 10,000; 1,000 smart
folders over one million items; unique smart predicates over 100,000; and FTS tests at 200,000.
These use optimized Rust builds. Native source was unchanged between phases, so timing variation
must not be attributed to the frontend changes.

| Scenario | Baseline p95 ms | Candidate p95 ms |
| --- | ---: | ---: |
| Warm All imported-order jumps | 4.61 | 3.66 |
| Warm name-order jumps | 41.90 | 39.45 |
| Warm random-order jumps | 43.74 | 39.93 |
| Warm common-tag smart folder | 6.28 | 6.05 |
| Warm rare-tag smart folder | 24.05 | 23.94 |
| Common text query | 6.13 | 7.74 |
| Mixed random-search jumps, 64 operations | 98.84 | 89.03 |
| Full traversal, 900 windows | 3.82 | 3.92 |

The million-row native fixture has 900,000 active roots; Inbox and Trash account for the rest.
Full active traversal increased from 2.52 to 2.76 seconds (+9.6%); cursor traversal decreased from
3.07 to 2.31 seconds. Hydration stayed bounded at 1,500 items, with 3.6 MB of compact ordered IDs.
Smart-folder projections remained 128,003,273 estimated bytes. Folder-edit p95 changed from 22.42
to 23.99 ms. No warm window case exceeded both the proposed 10% and 2 ms regression threshold in
this pair of runs. This is not a repeated statistical regression study or a whole-app memory test.

Cold common FTS still took approximately 592 ms; cold name-order jumping took 191 ms. Mixed query
cache churn still produced a 122 ms maximum jump. There is no universal 40 ms guarantee.

## Renderer CPU measurements

The reproducible script compares production spatial-index code from HEAD against the candidate,
alternating implementations over 400 measured samples after warm-up. It uses 1,500 positions.

| Spatial index build plus lookup | Baseline p95 ms | Candidate p95 ms |
| --- | ---: | ---: |
| Top | 0.046 | 0.048 |
| 4 million pixel offset | 0.190 | 0.043 |
| 7.9 million pixel offset | 0.592 | 0.063 |

Tail p99 fell from 2.637 to 0.094 ms. This is about 9.4 times faster at p95 for this small operation,
not a 9.4-times-faster application. Top-of-grid cost is effectively unchanged in absolute terms.

With canonical Lab color metadata, 100 measured 1,500-item layouts per mode had cold layout p95 of
1.22–1.45 ms. Preparing them cost 1.32–1.46 ms; consuming the prepared layout cost 0.0012–0.0028 ms.
Preparation moves bounded work before commit; it does not eliminate that CPU work. Measurements
exclude browser painting, IPC, image decoding, and end-to-end frame scheduling.

## Windows application smoke

Used only the existing synthetic Million Grid Test library (1,000,000 active items), an isolated
application profile, targeted approved input, and a ten-minute process watchdog. Read-only CDP
observations inspected grid state and existing IPC diagnostics. The installed real-library app
was not controlled. Launcher PID 20924, Vite PID 50636, and Electron PID 35352 were confirmed closed.

Observed successful initial loading, rare-tag smart-folder navigation, a scrollbar jump to the
million-item tail, and Back restoration to the exact same physical scroll position (7,999,248 px).
The tail window started at 998,500 with 1,500 records. The folder preview retained query-head IDs
1,000,000 through 999,997 instead of using tail records. Screenshots showed rendered tiles.

One warm Back sample committed at 171 ms and began fade-in at 188 ms from intent. A warm smart-folder
sample began fade-in at 186 ms. A cold smart-folder sample began fade-in at 391 ms, beyond the fade
budget. The first direct tail fetch took 973 ms round trip, of which 967 ms was native. Existing
diagnostics locate the delay in native execution but do not distinguish its internal stages in
that live request; the cause still needs profiling, not an assumption that layout or IPC dominates.

The watchdog expired during the random-sort input attempt; random sorting was not visually
verified. Automated controller tests cover preparation and seed behavior, and native benchmarks
cover random ordering. These do not substitute for the missing application smoke.

TypeScript passed. The full frontend/Electron test suite passed: 191 files, 1,113 tests. Existing
React act warnings remain. Production changes after that run were comment/indentation-only.

## Remaining gates and next work

This implements a measured first slice, not the entire design plan. Remaining work includes
correlated native-stage profiling for cold live requests; rapid mixed filter/sort/navigation and
library-switch soak; random-sort application smoke; memory and frame-time distributions; stable
item-anchor geometry; and macOS/Linux application smoke. Preference ownership is separately tracked
in issue 30. The 170 ms readiness p95 and cross-platform release gates have not been demonstrated.

Do not introduce a full-million snapshot based on these results: bounded layout is a small cost,
while the observed cold native delay is much larger. Profile cold ordering, hydration, I/O, and
execution contention before choosing the next backend optimization.

Raw local artifacts: `artifacts/benchmarks/grid-preparation/{baseline,candidate}/`, `renderer.json`,
and `app-smoke.log`. Reproduce native runs with
`node scripts/benchmark-grid-preparation.mjs candidate` (or `baseline` on the baseline revision) and
renderer measurements with `node scripts/benchmark-grid-renderer.mjs`. The two fixtures intentionally
differ and their timings must not be directly equated.
