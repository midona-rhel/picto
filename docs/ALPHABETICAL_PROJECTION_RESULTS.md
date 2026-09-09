# Cold alphabetical windows: shared ordering projection

2026-09-05. Local, uncommitted implementation for [issue #48](https://github.com/midona-rhel/picto/issues/48).
This supersedes the alphabetical-ordering limitation in `COLD_SEARCH_RESULTS.md`.
It does **not** resolve the remaining first-touch metadata-read limitation or establish a universal
50 ms cold-view guarantee. No UI transitions, thumbnail dwell times, schema, or release changed.

## Implementation and ownership

SQLite remains canonical. Each open library builds one root-name projection in SQLite's exact
`name COLLATE NOCASE, root_id` order. A balanced tree has leaves of at most 256 entries and root-ID
bitmaps at its branches. A never-before-requested filter can count matching IDs in a subtree and
skip it, then inspect the leaves covering the requested ordinal window. Ascending and descending
cursor pages use the same projection; the old alphabetical SQL scans and per-query full-order
materialization have been removed. Recently Viewed retains its timestamp-order contract.

This avoids enumerating a complete matching order, but it is not constant-time access: bitmap
intersection costs and the number of leaves visited still depend on the selection. Sparse filters
can visit more leaves. All, Inbox, Trash, tags, smart folders and other scopes supply their existing
canonical match bitmaps; no new filter semantics or source-of-truth table was introduced.

Root creation, renaming, duplicate-name upgrades, grouping, detaching, ungrouping and deletion
settle affected entries inside the existing mutation transaction, before publication. They do not
wait for FTS settlement. Nodes are copy-on-write for concurrent readers; subsequent edits to a
write batch's owned nodes reuse bitmap storage. Unrelated writes retain the same index.

Structural history captures canonical names and refreshes affected entries on replay. It does not
retain historical alphabetical trees or charge the library-wide name index against the undo budget.
Both normal and checkpoint opens rebuild the projection from SQLite, without a migration or a
checkpoint format change. This shifts work to library initialization, not to the first filter.

## Million-item results

Windows, release native builds. The standard generated fixture has one million media items and
900,000 active roots. Baseline: `artifacts/benchmarks/cold-search/windows-million-final.log`.
Candidate: `artifacts/benchmarks/alphabetical/windows-million-final.log`.

| Measurement | Before | After |
| --- | ---: | ---: |
| Alphabetical cold middle, complete 1,500-item native window | 170.86 ms | 32.35 ms |
| Alphabetical middle ordering alone | 138.44 ms | 0.53 ms |
| Alphabetical cold tail, complete native window | see baseline artifact | 27.91 ms |
| Date-added cold middle | 5.70 ms | 3.76 ms |
| Size cold middle | 28.49 ms | 25.26 ms |
| Mixed 64-query arbitrary-jump p95 | 27.51 ms | 26.90 ms |
| Full 900-window traversal | 3.11 s | 2.80 s |
| Full traversal window p95 | 4.49 ms | 4.03 ms |
| Windows interleaved with imports, p95 | 7.49 ms | 4.93 ms |

The full walk checks every window against a cursor oracle. Import-time smart-folder mismatches:
zero. Mixed jumps reuse some predicates; they are not all cold global searches. Timings vary between
runs: the first candidate traversal was 3.45 s and its mixed-jump p95 was 35.87 ms. These are retained
in `artifacts/benchmarks/grid-preparation/alphabetical/`, not discarded as inconvenient results.

### Memory, startup and writes

The existing renderable million-item library adds an estimated **143,338,580 bytes (136.7 MiB)** for
the name projection. This is a resident, library-wide allocation, not a bounded query-result cache.
Longer names and different ID distributions change its size. It is not process RSS and does not
include the SQLite cache, the text projection, or other existing projections.

The isolated million-name index benchmark builds in **421.76 ms**, selects its tested windows in
**0.13–0.26 ms**, and updates a renamed key with a retained old snapshot in **0.30 ms p95**. Inserting
1,000 names into one owned batch takes **5.77 ms**. These are projection-only timings, not complete
SQLite mutation/publication timings. The synthetic fixture's shorter names use about 125 MiB.

Full library reopen time is measured separately and excluded from the query timer. Across 48
reopens, the mean rose from **1.60 s to 2.34 s** (candidate range 2.06–5.17 s). These are separate runs,
not an isolated attribution of every millisecond, but the additional startup work is real.

The first implementation regressed bulk ingestion: the 200,000-item FTS fixture's import phase was
76.87 s versus 26.99 s previously, and the million-root smart fixture took 193.58 s versus 125.28 s.
This exposed repeated subtree copies within a batch. The final implementation reuses nodes already
owned by the batch. Final imports take **30.68 s for 200,000 items** and **138.17 s for one million**,
about **14% and 10% slower than the preceding implementation**, respectively. The severe initial
regression is removed, not all write overhead. These results are recorded in `fts-200000-final.log`
and `smart-million-final.log`; initial slower measurements remain in the suite directory.

Final million-root smart-folder edit p95 at depths eight/three/one is **24.19/27.97/26.45 ms**, versus
**28.30/26.73/22.35 ms** before. Changing the common rule to reject/restore all roots takes
**143.21/86.83 ms**, versus **107.26/65.15 ms**. Some non-target operations are slower; this change
does not claim zero regressions. At 10,000 roots, batch-ingest p95 is 15.01 ms versus 15.73 ms, and
concurrent-read p95 is 2.47 ms versus 3.08 ms in the complete suite run.

### Remaining cold-read limitation

`cold_search_window_probe` reopens the renderable million-item library for each of 48 requests.
There are no reused match/order caches. Filesystem caches are not forcibly flushed. Requested
windows contain up to 1,500 summaries. Alphabetical results additionally match an independent SQL
ordering oracle, executed outside the request timer.

In the repeated run, only **30/48 complete native windows** were under 50 ms. Alphabetical ordering
took at most **2.64 ms**, but a common-name head request took **409.81 ms**, of which **399.06 ms** was
summary loading; its middle request took **397.99 ms**, with **387.35 ms** in summary loading. Earlier
candidate samples show the same bottleneck. Date, size and random order also retain metadata-read
outliers. This is not a claim of an end-to-end cold-view speedup for every request.

The query still reads scattered root, cover-media and file rows to construct the summary payload.
Removing name enumeration does not remove those reads. The next distinct performance problem is
the cold summary-read path, not alphabetical ranking. Warming a prior request must not be presented
as solving cold arbitrary lookup. Image decoding, rendering and completion of fades are outside
these backend timers.

### Final Electron check: correctness passes, latency target fails

The rebuilt Windows debug app was tested with distinct first-time global-search terms after
library initialization. All 11 counts/window bounds pass, two grid canvases mount, and the owned
Electron/Vite/launcher processes exit. Only **4/11 requests are below 50 ms round trip**.

| First-time search / destination | Previous slice round trip | Final round trip |
| --- | ---: | ---: |
| Common text, name head (500 items) | 29.60 ms | 190.70 ms |
| Common text, name middle (1,500 items) | 253.10 ms | 508.00 ms |
| Common text, name tail (1,500 items) | 295.80 ms | 493.00 ms |
| Common text, random middle (1,500 items) | 57.20 ms | 438.50 ms |

These end-to-end measurements are **worse**, despite the independently measured ordering gain.
The earlier candidate app run also missed the target: name middle/tail were 277.90/231.10 ms.
Native stage measurements consistently expose cold summary reads; the Electron round-trip timer
alone does not attribute every millisecond. Filesystem state and system-load variation were not
controlled, so neither a causal attribution of all regressions nor a warmed-run speed guarantee
is justified. Preserve both `electron-first.json` and `electron.json`. The 50 ms performance
acceptance criterion remains unmet; cold metadata loading needs attention before treating this
as a complete application performance fix.

## Verification

- 117 native unit/integration tests pass; the separate million-name index test and FTS scale test
  also pass. Clippy for all library targets passes with warnings denied.
- SQL parity covers ASCII case ties, Unicode, empty strings, embedded NUL, both directions, sparse
  and empty selections, head/middle/tail windows, cursor paging, tree splits/merges and old snapshots.
- Application-level tests query before FTS settlement after root mutations, exact-hash name upgrades,
  duplicate resolution, undo/redo, lifecycle changes, permanent deletion and checkpoint reopen.
- All six existing benchmark groups pass: grid, complete windows, general operations, million-root
  smart folders, unique-filter smart folders and the 200,000-item FTS suite.
- Windows Electron query-only smoke validates first-time query counts/window bounds and mounted
  grid canvases in the fake library. Owned processes are closed afterward. This does not automate
  navigation input or claim visual fade/image-decoding coverage. macOS and Linux smoke are not run
  on this Windows host.

Artifacts: `artifacts/benchmarks/alphabetical/` and
`artifacts/benchmarks/grid-preparation/alphabetical/`.
Reproduce the suite with `node scripts/benchmark-grid-preparation.mjs alphabetical` and the direct
index probe with `cargo test --release --offline -p picto_library --lib million_name_index_build_and_incremental_writes -- --ignored --nocapture`.
