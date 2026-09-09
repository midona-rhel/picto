# Grid preparation performance plan

Status: viewport-first implementation and baseline/candidate benchmarks completed locally on
2026-09-05; broader profiling, native improvements, and cross-platform gates remain unfinished.
See `GRID_PREPARATION_RESULTS.md` for measured results and explicit validation gaps.
This is a design and validation plan, not an additional executable backlog. Accepted implementation
slices belong in `RELEASE_COMPLETION_PLAN.md` and their tracking issues.

## Outcome and visual contract

Make the correct destination viewport ready during the existing transition. Improve actual query,
transfer, layout, and drawing costs; do not redesign the UI or lengthen animations to hide delays.

Priority clarified on 2026-09-05: optimize cold searches and the first arbitrary jump into their
results first. Measure matching, ordering, hydration, and delivery together. Warm-cache timings
are secondary regression checks, not the primary success metric; distinguish cold query caches
from cold OS/storage caches explicitly.

- Preserve navigation fade-out and fade-in: currently 170 ms each, with their existing easing.
- Preserve primary-color-to-thumbnail reveal and size-dependent HQ promotion. The current thumbnail
  reveal envelope is 500 ms; it is separate from navigation timing.
- Keep deterministic visibility eligibility: thumbnails at 100 ms, HQ at 250 ms, including cached
  content. Preparing a hidden destination must not count as time visibly on screen.
- No new spinner, progress bar, debug overlay, skeleton design, setting, or interim result set.
- Preserve current inline filter behavior; do not introduce a new transition for filters in this work.
- No blank first frame for a nonempty result, wrong-page flash, late label/count pop-in caused by
  incomplete metadata, or scroll jump when background preparation finishes.
- Preserve mixed-media tiles, collection counts, selection, hit testing, and the first four items
  of the actual query for folder previews, independent of the loaded window.

The deadline is **destination first-draw readiness at the 170 ms fade midpoint**, not loading or
decoding one million images in 40 ms. Cold arbitrary filters cannot honestly have a universal
40 ms guarantee. A missed deadline remains a measured delay using the existing waiting behavior,
not an excuse to extend the fade or show guessed results.

## What exists already

| Area | Current behavior | Opportunity |
| --- | --- | --- |
| Navigation | `WorkspaceSurface.tsx` prefetches during fade-out | Prepare geometry as well as data, and target the restored viewport |
| Sort changes | Intent is applied after fade-out | Prepare the incoming query while the outgoing surface fades |
| Controller | First page is 500; windows are bounded at 1,500; preferences are awaited before querying | Resolve/cache preferences safely and avoid fetching the head before a distant destination |
| Native query | Reuses matches and ordered IDs; random windows use a seeded permutation | Measure remaining hydration/serialization and cold order construction, not add another order cache |
| Layout | Resident items become objects, positions, an ID map, and a spatial index on the renderer thread | Keep this bounded; do not feed it a million records |
| Layout math | Shared grid/waterfall/justified algorithms already have continuation support | Reuse one algorithm; account for unfinished justified rows and waterfall column state |
| Scrolling | 40 ms settle delay; estimated extent; physical height capped at 8,000,000 px | Preserve arbitrary access and the cap; improve restoration without visible correction |
| Drawing | Retained canvas buffers, separate base/overlay, coalesced animation frames | Preserve reuse; background metadata completion must not trigger unrelated repaints |

Source owners: `src/controllers/gridController.ts`, `src/features/workspace/WorkspaceSurface.tsx`,
`src/features/grid/canvas/`, `src/features/grid/layout/`, and `picto-library/src/query.rs`.

## Chosen sequence

### 1. Establish an end-to-end baseline before changing the data model

Add correlated, development-only timings using library session and request generation:

- Intent received; preferences ready; query queued and started.
- Match, aggregate, order, summary hydration, encoding, and transfer costs separately.
- Renderer receipt, layout readiness, restore readiness, first base draw, and fade-in start.
- Deadline overrun, canceled work duration, main-thread long tasks, repaint counts, and peak memory.

Use durations within each process and a shared request ID; do not subtract unrelated process clocks.
No per-tile production logging and no visible instrumentation. Separate initial library open,
cold query on an open library, warm query, cold image cache, and warm image cache.

Deliverable: reproducible baseline artifacts and the largest contributors for each scenario. This
determines whether summary hydration, execution contention, transfer, or layout gets the next fix.

### 2. Prepare the actual destination while preserving the current transition

Extend the existing prepare/commit navigation path to sort changes as well. Do not add a second
query implementation for prefetch. Preparation must not mutate the visible session or persist
preferences as if the user had already committed the destination.

At intent receipt:

1. Capture library session, query identity, sort seed, view preferences, layout dimensions, and
   restore target. Cache preferences by library and scope, with explicit invalidation; never borrow
   settings from another scope to launch a speculative query.
2. Start the destination query immediately once those inputs are available. On ordinary navigation
   prioritize the first window. On Back/Forward prioritize the saved viewport, not page zero.
3. Prepare visible tile metadata and layout while the old grid fades. Fetch the query's four-item
   preview independently; do not download all intervening windows to obtain it.
4. At the existing midpoint, commit only the current generation's coherent query/layout result.
   Restore position before exposure; signal first-paint readiness only for that destination.
5. Start the existing fade-in. Image dwell begins when the incoming surface becomes visible;
   invisible preparation cannot advance reveal state or bypass decode admission.

A meaningful first draw contains correct visible IDs, geometry, primary colors, enabled text and
badges, hit-test data, and selection state. It does **not** wait for thumbnail decoding or HQ.
Empty and error results use their existing UI paths, never stale tiles from the previous query.

Rapid navigation supersedes queued and running preparation. A stale response cannot commit,
restart work, or access a newly opened library using old local item IDs. Bound pending work to the
latest destination; do not build a queue of obsolete navigation requests.

### 3. Reduce native work before increasing the renderer's working set

Keep SQLite authoritative and existing bitmap/query caches as derived data. Profile the remaining
window hydration and return only the canonical fields needed for the grid's current display.
Ratios and colors alone are insufficient when names, media kinds, covers, or collection counts are
visible. Details and image bytes remain demand-loaded.

Reuse existing projections for layout scalars where available. Add a derived field only when the
baseline demonstrates its benefit, its owner and memory cost are explicit, and ordinary writes can
update affected entries incrementally. No second authoritative grid table or duplicate filter engine.

Keep relevant query dependencies in cache identity. Unrelated writes should reuse matches/order;
relevant changes must invalidate or update affected data before a coherent replacement is exposed.
Never combine order from one snapshot with descriptors from another without validating identity.

Visible-window work outranks speculative warm-up. Execute blocking work through a bounded,
cancellable backend path, not on an async event worker. Avoid holding a database connection,
transaction, or library lock across IPC delivery or background layout. Check cancellation between
bounded chunks; dropping a renderer promise is not cancellation of native work.

Inspect the previously recorded command-execution and shutdown issues when touching these paths;
do not make this change depend on solving every unrelated architectural review finding.

### 4. Precompute more only if measurements justify it

Prototype compact background preparation after steps 1–3. The first draw must never depend on a
whole-million transfer, full layout, or completion of a speculative adjacent window.

Try bounded descriptor chunks first. Keep authoritative query ordering native and request only
the ranges needed now; retain the existing direct random-window lookup. A compact full-query
snapshot is a **benchmark candidate**, not the default design decision.

For that candidate:

- Store IDs, ratios, and packed color/flags in typed buffers, with the actual ID width preserved.
  Keep strings, media URLs, and rich tile objects restricted to resident windows.
- Measure the complete native-to-Electron-to-renderer-to-worker transport. A typed array at the
  end does not make earlier JSON serialization or intermediate copies free.
- Use one layout worker running the shared layout algorithm, separate from the image decoder.
  Return bounded visible geometry to the renderer; do not materialize a million React items,
  object positions, or per-item string maps on its main thread.
- Fixed-cell grid positions can be calculated directly. Waterfall needs column-height checkpoints;
  justified layout needs row-boundary checkpoints and provisional final-row handling. Do not
  claim cold exact arbitrary geometry without computing the necessary predecessor state.
- Replace repeated whole-prefix copying in any streaming layout implementation; merely calling
  the existing append helper repeatedly would still copy growing position arrays.
- Enforce explicit byte and task limits, including native caches, transport copies, worker buffers,
  active plus incoming sessions, history retention, and image/protocol caches. Initial prototype
  ceiling: 128 MiB additional compact-data/layout memory per library, excluding existing image
  caches but including in-flight copies. Exceeding it stops speculation, never the visible request.
- Keep only bounded warm history. Do not retain a million-item snapshot for every filter or sort.
  No background image decode: this warm-up prepares metadata and geometry only.

Promote this candidate only if repeated arbitrary jumps improve enough to justify its total CPU,
memory, and cancellation costs. Otherwise ship the measured viewport-first improvements without
the full snapshot. Remove any temporary comparison path before landing production changes.

### 5. Stable geometry and meaningful drawing

Identify prepared layouts by query/order identity and geometry inputs: width, tile size, layout
mode, gap, text height, and scrollbar width. Resize or display-setting changes supersede incompatible
layout work without discarding still-valid query results.

Retain a logical item anchor plus relative viewport offset for history restoration. Resolve it in
the destination query; if removed, choose the nearest surviving position and clamp at boundaries.
Do not rely exclusively on scroll percentage or assume a resident index is a query ordinal.

Never swap independently packed windows into globally packed geometry and assume they line up.
Use matching checkpoints and complete rows, or retain the committed geometry until a consistent
handoff is possible. Test that background completion cannot move the visible anchor, change the
scroll thumb under a drag, or restart image fades. Preserve bounded physical scrolling for logical
heights beyond 8,000,000 px; use sufficient coordinate precision at the million-item tail.

Retain the existing canvas scheduler and base/overlay split. Decode completion and animated reveal
can dirty visible base content; hover/selection can dirty the appropriate overlay. Offscreen warming
must not request a redraw. Do not add a permanent animation loop or scan a million entries per
pointer event, image completion, or frame. Keep visible metadata lookup and hit testing bounded.

## Ownership boundaries

| Owner | Owns | Must not own |
| --- | --- | --- |
| Native library/query layer | Canonical membership, ordering, query caches, bounded descriptor reads | Navigation animation or renderer-specific speculative inserts |
| Grid controller | Query identity, destination preparation, cancellation, atomic session commit | Canvas painting or a second filter implementation |
| Workspace surface | Existing transition phases and destination visibility | SQL, full-query materialization, or image decode policy |
| Shared layout kernel / optional worker | Geometry, continuation/checkpoints, bounded layout cache | Library truth or a duplicate Rust/TS packing algorithm |
| Canvas renderer | Visible drawing, interaction, retained pixels, first-draw acknowledgement | Full-library hydration or speculative background repainting |
| Image pipeline | Visibility dwell, decode admission, bounded bitmap cache, reveal | Advancing timers for hidden prepared destinations |

## Validation and gates

Use the existing Million Grid Test library only for application smoke, without changing its data.
Use disposable fixtures for writes and scale tests. Include both repeated-content synthetic data
and distinct file identities; a million rows sharing a few images is not an image-cache stress test.

Run and extend the existing suite, not a replacement benchmark that only measures the new path:

- `grid_query_scale_probe`: 10k, 100k, and 1m; query plus serialization.
- `grid_window_scale_probe`: complete traversal, seeded random jumps, and smart-folder windows.
- `scale_probe`, `smart_folder_scale_probe`, and `fts_scale`: existing general, smart-folder, and
  FTS cases at their configured sizes, including common and rare predicates.
- Renderer/controller/layout tests and application timing traces for the same requests end to end.

Cover All/Inbox/Trash separation, folders, smart folders, common/rare tags and FTS, combined filters,
random order with stable seeds, all layouts, and collections. Exercise top/middle/end jumps, filtering
while at the end, Back/Forward to that position, rapid destination changes, resize/zoom, and empty
results. Test relevant and unrelated writes during preparation, library switching/closing, slow I/O,
errors, cancellation, and memory eviction. Selection and folder previews must remain query-correct.

Report p50/p95/p99 with sample counts and outliers, not just an average or best run. Use release
builds for performance comparison and debug builds for diagnosis. Record fixture seed, hardware,
build, cache state, and working-set size. Collect at least 100 warm navigation/jump samples per
critical scenario; treat cold samples separately and disclose their smaller sample counts.

Proposed gates, to be checked against the baseline rather than claimed as current results:

- Warm common destinations: first-draw-ready p95 within 170 ms of intent receipt.
- Warm native window queries: p95 below 40 ms; report transfer/layout separately.
- No new renderer tasks above 50 ms; investigate frame-budget overruns at both 60 and 120 Hz.
- No reproducible p95 regression exceeding both 10% and 2 ms in unaffected query scenarios.
- No increased blank interval, extra repaint stream, broken dwell/fade, or anchor movement.
- Cancellation and memory limits must hold under repeated million-item query changes, not just
  one navigation. Measure interference with imports and smart-folder updates as well.

Run focused tests after each coherent slice, then the existing verification suite and application
smoke on Windows, macOS, and Linux. Compare transition recordings and visible tile behavior before
and after. Read-only CDP observation is allowed; obtain explicit approval before automated UI
driving, with bounded process lifetime and guaranteed cleanup.

## Delivery order and completion

1. Baseline instrumentation and failing behavioral tests for preparation/restoration gaps.
2. Destination-aware query preparation during the existing fade, with safe cancellation and commit.
3. Measured native hydration/execution fixes; rerun the entire benchmark suite.
4. Only if needed: bounded worker layout and compact-snapshot prototype, with a go/no-go report.
5. Cross-platform smoke, memory/interaction soak, removal of superseded paths and temporary probes.

Track accepted fixes with issues and reference them in implementation commits. No release is implied
by this planning step. Completion requires evidence of better readiness with unchanged visual
behavior, not merely a faster isolated SQL query or work moved into an unmeasured background task.
