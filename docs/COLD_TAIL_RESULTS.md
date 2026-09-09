# Cold tail window fix

2026-09-05. Tracking: https://github.com/midona-rhel/picto/issues/46.
Local implementation; not committed or released.

Historical measurement slice: the later cold-search work in `COLD_SEARCH_RESULTS.md`
supersedes the date/size middle-order path described here; these are the earlier results.

## Cause and fix

`items.window` previously built the entire matching order on its first non-head read, including
a request for only the final 1,500 items. The supplied fake million-item library reproduced a
901.9 ms date-added tail query: 892.0 ms ordering versus 9.9 ms summary hydration.

The cold tail now reads from the opposite end using the existing canonical pager, then reverses
the bounded result into the requested order. Both the sort value and root-ID tie-breaker reverse.
It applies to indexed date-added, name, and size sorts within the final 3,000 matching positions.
At most 1,500 summaries are hydrated. Existing matching bitmaps, exact totals, revisions, filters,
query cancellation, and mutation settlement remain authoritative.

Existing cached orders still serve arbitrary windows. Cold middle reads still build compact
orders. Captured-date expression sorting, manual folder order, recently viewed, and random order
retain their existing semantics and paths. No schema migration, new projection, full-renderer
snapshot, or UI/fade change is involved.

## Measurements

The diagnostic `cold_window_probe` reopens the supplied library for each sample, then reads its
head before the cold tail. This makes the query-order cache cold, not necessarily the OS disk cache.
Before and after runs were sequential; disk caching was not flushed. Do not treat the largest
first-sample speedup as a controlled universal latency guarantee.

| Fake library, 1,000,000 active items | Before, three samples ms | After, three samples ms |
| --- | --- | --- |
| Date added | 901.9 / 140.9 / 170.4 | 7.0 / 8.0 / 6.6 |
| Name | 2416.2 / 192.4 / 205.9 | 42.8 / 34.4 / 27.9 |
| File size | 812.5 / 131.0 / 131.3 | 9.8 / 10.9 / 10.3 |

Every optimized sample built zero full orders. Date-added ordering alone fell to 0.77–0.85 ms;
the returned tail endpoint IDs remained identical. The debug profile uses the repository's
configured optimization overrides for application crates.

The optimized million-item `grid_window_scale_probe` was also rerun. Its fixture contains 900,000
active roots and 100,000 Inbox/Trash roots. Comparison is against the immediately preceding
preparation-only candidate run, with the same fixture construction and existing warm workload.

| Cold tail scenario | Before ms | After ms |
| --- | ---: | ---: |
| All, date added | 90.92 | 3.50 |
| Name | 191.28 | 28.55 |
| File size | 125.56 | 24.21 |
| Common-tag smart folder | 112.12 | 4.42 |
| One-percent-tag smart folder | 123.79 | 50.15 |
| Common-text smart folder | 90.81 | 6.28 |

Warm date-added jump p95 changed from 3.66 to 3.81 ms; random-order p95 from 39.93 to 36.12 ms.
Warm smart-text-rare p95 increased 20.38 to 21.82 ms, and smart-text-common 6.28 to 7.18 ms.
No warm case exceeded both the proposed 10% and 2 ms regression threshold in this pair of runs.
The random-order single cold sample increased 30.97 to 34.78 ms; its implementation is unchanged.

Full traversal of 900,000 active items over 900 windows took 2.21 seconds, versus 2.76 seconds before;
window p95 was 2.91 versus 3.92 ms. Sixty-four mixed random searches had jump p95 88.85 versus
89.03 ms, but maximum increased 122.38 to 137.91 ms. Rating writes preserved match/order caches.
Tail reads after imports fell from 116.05 to 5.10 ms p95; all smart-folder comparisons matched.
These are individual benchmark runs, not a repeated statistical regression study.

The benchmark now reports cold middle timing separately before warm jumps, because a tail read
no longer warms the complete order. Cold middle queries remain 77 ms for date-added and 152 ms for
name in this run. Initial common FTS matching still took 530 ms. This fixes cold tails, not every
cold arbitrary lookup or the first evaluation of a broad text filter.

## Verification

- Library unit/integration suite: 109 tests passed; the opt-in FTS scale test was not rerun.
- Clippy passed for all library targets with warnings denied.
- Dense and sparse cursor/window parity covers ascending/descending order, case-insensitive name
  ties, lifecycle boundaries, folders, smart folders, mutation invalidation, and boundary windows.
- Native Electron module rebuilt locally. Query-only Electron IPC smoke passed on the fake library.
- Platform-neutral Rust/SQLite implementation; macOS and Linux application smoke not run here.

The isolated Electron app returned the cold date-added tail in **38.8 ms round trip / 29.2 ms
native**, name in 92.2 / 82.4 ms, and size in 32.6 / 25.0 ms. Each returned exactly 1,500 records
at ordinal 998,500, matched an independently paged reverse-order reference, and left the head
unchanged. These are single application samples, not p95 values; they include more overhead and
startup interference than the standalone probe. The grid was mounted with two canvas layers.
No synthetic UI input was used. The bounded check closed its owned Electron and Vite processes.
Details: `artifacts/benchmarks/cold-tail/electron.json` and `electron.log`.

Raw new benchmark output: `artifacts/benchmarks/cold-tail/windows-million.log`.
Previous comparison: `artifacts/benchmarks/grid-preparation/candidate/windows-million.log`.
Reproduce with `cargo run --release --offline -p picto_library --example grid_window_scale_probe`.
To inspect an existing fake library without fixture mutations, run
`cargo run --offline -p picto_library --example cold_window_probe -- <library.sqlite>`.
