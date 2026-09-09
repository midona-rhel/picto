# Cold search and ordinal window investigation

Historical baseline for the text/numeric slice. The later [alphabetical projection results](ALPHABETICAL_PROJECTION_RESULTS.md)
supersede the name-ordering limitation below; cold summary-read work remains unfinished.

2026-09-05. Local, uncommitted work. Tracking: [implemented slice #47](https://github.com/midona-rhel/picto/issues/47),
[remaining alphabetical and hydration work #48](https://github.com/midona-rhel/picto/issues/48).

## What changed

- Search uses an immutable packed projection of **settled SQLite FTS text**, not a cache of prior
  query results. Global, name, notes and URL searches retain exact trimmed, lowercased substring
  semantics, literal wildcard handling and the three-character minimum. Field boundaries remain
  separate. The projection uses at most four shared workers for large selections.
- The corpus is loaded at library open. Only affected 1,024-ID shards are refreshed in the existing
  FTS settlement transaction, before smart-folder evaluation and publication. Unrelated writes do
  not reload it. Removed roots are masked immediately; structural history preserves the current
  settled corpus rather than restoring historical search text. Checkpoints rebuild this projection
  from SQLite, without changing the on-disk schema or checkpoint encoding.
- The corpus has a 128 MiB admission budget. Libraries over that budget, or containing embedded NUL
  text with SQLite-specific LIKE behavior, retain the exact SQL implementation. This explicitly
  means the fast path and a sub-50-ms search are **not guaranteed for every library**.
- Date-added and size windows select an ordinal range from existing numeric bitmaps instead of
  enumerating the complete matching order. Shared merged bit slices are invalidated by writes;
  matching-set cardinality skips irrelevant branches. Small buckets finish without per-item bitmap
  allocations. Ties retain the canonical ID direction. Signed imported dates that would lose
  information in the existing clamped numeric projection retain SQLite ordering.
- A head window of 1,500 items no longer builds a complete name order. Tile reads are grouped by
  root ID for locality, then returned in the exact requested display order.
- Debug builds optimize the Roaring and SQLite dependencies as well as Picto's own crates, matching
  the existing policy for image-processing dependencies. This changes debug performance, not the
  release implementation, and does not remove application debug information.
- No frontend transition, thumbnail/HQ admission, renderer payload shape or visible result was changed
  by this slice. Earlier preparation work remains separately documented.

## Measurement boundaries

Host: Windows, AMD Ryzen 7 5800X3D (8 cores / 16 logical processors). Native benchmark binaries use
release optimization; the Electron check uses the local debug build with repository optimization
overrides for Picto crates. Do not equate those two timing populations.

The existing six-part benchmark suite ran in release mode: 10k/100k/1m grid queries, million-item
windows and complete traversal, general reads/writes, 1m roots with 1,000 smart folders, 100k roots
with 1,000 distinct smart-folder filters, and the opt-in 200k-item FTS settlement test. All passed.
Artifacts are under `artifacts/benchmarks/grid-preparation/cold-search/`; the preceding comparison
run is `candidate/`. The general grid fixture has one million media rows, 900,000 active roots and
100,000 Inbox/Trash roots. It is not an image decoding or real-image working-set benchmark.

`cold_search_window_probe` separately reopens the renderable million-active-item fake library for
each of 48 first-time global-search windows: four terms, four sorts, head/middle/tail. It requests
1,500 summaries, including head requests, so it does not hide hydration behind a smaller payload.
No matching or order cache survives reopening. **OS filesystem caches are not flushed.** Projection
loading happens before these timers and is reported separately. These are individual cold samples,
not a statistically established p95 for every possible query or hardware configuration.

The packed corpus in that fixture occupies 52,350,812 bytes (49.9 MiB), in addition to existing
projections and lazy merged numeric slices. It adds no database index or schema migration. The
prototype loaded text in roughly 0.6 s; production reports total library opening separately in
`native.json`, not as part of a first query. Startup cost must not be mistaken for eliminated work.
Final total library opening ranged from 1.29 to 1.75 seconds (mean 1.60 seconds).

## Results and remaining costs

The existing release suite's cold common **name-text** query fell from 769.55 ms to 12.43 ms, including
500 summaries. Cold rare name-text fell from 30.35 to 23.67 ms. General 10k ingest batch p95 went from
17.92 to 15.73 ms; FTS settlement for 200k rows went from 22.04 to 19.63 seconds. These are paired
benchmark runs, not claims of guaranteed improvements on every machine.

The full 50 ms end-to-end target is **not achieved**:

The final reopened-library run had **46/48 native windows below 50 ms**:

| Sort | Samples below 50 ms | Worst native window ms |
| --- | ---: | ---: |
| Date added | 12/12 | 40.99 |
| File size | 12/12 | 35.93 |
| Random | 12/12 | 43.58 |
| Name | 10/12 | 264.68 |

The name misses were the common-term head (264.68 ms, including 219.35 ms ordering) and middle
(179.44 ms, including 137.36 ms ordering). Matching stayed below 19 ms in every sample. Filesystem
pages had been touched by previous runs: fresh query caches are not the same as cold disk. Earlier
first-touch observations included a 343.52 ms rare-date window (332.06 ms hydration) and a 241.70 ms
common random window (231.45 ms hydration). Those outliers are not erased by the faster final pass.

Remaining costs:

- Alphabetical middle windows still enumerate the matching order. Filtered alphabetical head/tail
  scans may also cross a long nonmatching prefix. An ordinary index seek does not skip that work.
- Cold scattered summary reads still join root, cover media and file tables. First-touch 1,500-item
  windows can spend hundreds of milliseconds here even after matching and ordering become fast.
- The projection budget fallback, signed-date fallback, other sorts, complex predicates, disk
  latency and IPC scheduling prevent an honest universal latency ceiling.

Final million-row ordinal-window run (900,000 active roots):

| Scenario | Cold middle ms | Warm jump p95 ms |
| --- | ---: | ---: |
| Date added | 5.70 | 4.29 |
| File size | 28.49 | 34.75 |
| Name | 170.86 | 34.31 |
| Common name-text | 5.95 | 6.83 |
| Common-tag smart folder | 5.65 | 6.95 |

The earlier cold-tail slice measured date-added cold middle at 77 ms. The mixed 64-query jump
workload now has p95 27.51 ms and maximum 32.97 ms. This workload includes reused predicates, so
these numbers must **not** be relabeled cold global-search timings. Import-time windows have
p95 7.49 ms with zero smart-folder mismatches.

The complete 900-window traversal is **slower** than the preceding cold-tail run: 3.11 versus
2.21 seconds; p95 4.49 versus 2.91 ms. Numeric ordinal selection trades roughly 1 ms per window
for eliminating the first full-order construction. This remaining cost is not hidden by the
large improvement in cold middle queries. The first attempted implementation was worse (4.21 s);
the final version removes repeated range construction and tiny per-item bitmap allocations.

Some non-target measurements are slower: the first suite run's name-page p95 increased from 12.77
to 15.45 ms; million-root smart-folder edit p95 at depths eight and three increased from 24.48/22.25
to 28.30/26.73 ms. Other smart-folder depths improved. These differences need repeated controlled
comparison before attributing them to the change; this report does not claim zero regressions.

Final traversal, reopened-window and Electron timings are recorded in
`artifacts/benchmarks/cold-search/windows-million-final.log`, `native.json`, and `electron.json`.
Earlier intermediate traversal runs exposed repeated numeric range work and tiny allocations;
the final run includes their removal. Do not substitute warmed search timing for these cold results.

### Final Windows Electron debug smoke

Each row uses a distinct first-time global-search term. The library projection is already loaded;
round trip includes IPC and return of the bounded summary payload, not image decoding or a completed
visual fade. These timings are after optimizing the debug Roaring/SQLite dependencies. The previous
debug run is retained in `electron-before-optimized-deps.json`; filesystem caching and run-to-run
variation mean this is not an isolated statistical attribution of every millisecond to compiler flags.

| Search / destination | Items | Native ms | Round trip ms |
| --- | ---: | ---: | ---: |
| Common text, date head | 500 | 20.00 | 33.80 |
| Common text, date middle | 1,500 | 39.72 | 48.80 |
| Common text, date tail | 1,500 | 32.05 | 39.70 |
| Common text, name head | 500 | 26.84 | 29.60 |
| Common text, name middle | 1,500 | 245.53 | 253.10 |
| Common text, name tail | 1,500 | 289.42 | 295.80 |
| Common text, random middle | 1,500 | 50.16 | 57.20 |
| Common text, size middle | 1,500 | 35.23 | 43.40 |
| Rare text, date head | 500 | 21.22 | 24.70 |
| Numeric substring, date middle | 1,500 | 28.18 | 34.50 |
| Literal wildcard, empty result | 0 | 9.05 | 9.30 |

Eight of eleven app requests were below 50 ms round trip. All expected counts and window bounds
passed, two canvases were mounted, and the owned test processes exited. This is a query-only
application smoke, not automated visual navigation coverage.

## Verification

- 115 native unit/integration tests passed, plus the separately run FTS scale test.
- Exact SQL parity covers Unicode, literal wildcards, field boundaries, budget fallback, sparse and
  parallel scans, copy-on-write snapshots, settlement, collection history and checkpoint reopening.
- Numeric windows match canonical cursor order across scopes, directions, signed-date fallback,
  ties, missing IDs and mutation invalidation. Full traversal checks every returned window against
  the cursor oracle; import-time smart-folder comparisons remain exact.
- Native module built on Windows. Query-only Electron smoke uses distinct never-before-requested
  search terms, validates counts/window bounds, and closes its owned Electron/Vite processes. It
  does not drive UI input or access the real library.
- macOS/Linux application smoke was not performed on this Windows host. Shared Rust implementation
  and portable dependencies are not substitutes for those platform checks. No release was made.

Reproduce with `node scripts/benchmark-grid-preparation.mjs cold-search`, and
`cargo run --release --offline -p picto_library --example cold_search_window_probe -- <synthetic-library.sqlite> <output.json>`.
