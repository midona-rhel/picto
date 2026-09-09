# Simplifying Picto's browsing model

Status: implemented in `picto-library` on 2026-09-07; `docs/architecture.md` describes the
shipped model. Deviations from this proposal are recorded in the closing section.

## Decision in plain language

Picto should prepare a compact, resident model of an open library and browse that model. SQLite remains the durable authority; it should not have to supply drawing metadata for each grid window.

We accept somewhat slower opening and ingestion, and more memory use, in exchange for predictable interaction and substantially simpler ownership. Maintainability wins over a small performance improvement that requires more execution modes, synchronization, or fallback behavior.

The user's mention of five gigabytes was an illustration, not a memory limit. The earlier one-gigabyte browsing target and fixed startup target must not silently dictate this design. Measure real consumption and responsiveness, then judge the tradeoff. Do not promise that every exact search becomes instantaneous merely because its data is in RAM.

This proposal authorizes no implementation, database migration, test deletion, commit, or release by itself.

## 1. The problem we are actually solving

The current implementation already keeps much of the library in memory: membership bitmaps, numerical ordering data, alphabetical structures, collection ownership, and some searchable text. It is not a disk-only backend.

The complexity comes from an incomplete resident model and the mechanisms bridging its gaps:

- Drawing summaries have an independent persisted representation, a page cache, generation/checksum revalidation, per-query recovery, and optional helper connections.
- Search text is admitted or rejected as a whole. Budget rejection or exceptional text changes the execution path to SQL and several selection-dependent strategies.
- A grid query coordinates a SQLite transaction with an in-memory projection snapshot, even when most of its work already happens in memory.
- Multiple derived formats and settlement paths must agree after edits, imports, grouping, and history replay.
- Tests increasingly describe those mechanisms rather than the user-visible requirement they serve.

Some of this work was useful: coherent prepared windows, correct arbitrary jumps, candidate-aware filtering, stable previews, incremental updates, and transactional history should survive. The goal is not to undo everything. It is to stop requiring two storage worlds to cooperate on every normal browsing read.

The measured cold drawing-data delay demonstrates the cost of demand loading. Its precise current speed is a measurement question; a newly optimized loader is not a reason to preserve that architecture indefinitely.

## 2. Product rules versus implementation choices

Tests are evidence of intended behavior, not independent product authority. Existing behavior is the compatibility baseline, but an accidental implementation detail does not become a permanent requirement because an agent wrote an assertion for it.

### Product rules to preserve

- Files, logical media, visible roots, and collections retain their distinct identities and ownership.
- Library scopes, filters, smart-folder rules, sorting, and exact counts remain correct.
- A folder preview contains the first four matching items in query order, independent of the currently loaded window.
- Arbitrary jumps and back navigation retain their established position semantics.
- A prepared result contains items, preview, counts, and position from one revision. Superseded navigation cannot replace the current destination.
- Existing navigation fades and primary-color-to-thumbnail-to-HQ behavior remain intentional presentation features.
- Committed edits survive reopening; failed edits do not partly appear; undo does not erase unrelated subsequent work.
- Libraries do not mix across a session switch, and background work is owned and stopped when its session closes.

### Implementation choices that may change or disappear

Page sizes, candidate thresholds, reader counts, cache classes, exact query counts, internal generation counters, node formats, batching sizes, and the number of helper threads are not product rules.

Some implementation properties still deserve tests while their mechanism exists: permits must be returned, memory ownership must be sound, and a checkpoint must not be used for the wrong revision. Such tests protect engineering invariants; they must not prevent replacing the mechanism itself.

### Ambiguous behavior

Do not silently preserve or silently change quirks such as special NUL matching, missing-date sentinel ordering, or random-order reshuffling after membership changes. State the observable behavior and its provenance. Preserve it during this refactor unless a product decision explicitly changes it. A request to simplify the backend is not permission to redefine search or sorting.

## 3. One resident browsing model

Extend and consolidate the existing `ProjectionStore` / `ProjectionSnapshot` rather than building another authoritative cache beside them. One published snapshot represents all browsing-relevant state for one library revision.

The following are responsibilities, not a demand for a new class or framework for each row:

| Responsibility | Owner | Must not own |
| --- | --- | --- |
| Durable records and transactions | SQLite / existing database owner | Grid-specific rendering state |
| Resident records, memberships, search and order indexes | One library model owner | Another independently updated copy of the same facts |
| Filtering, ordering and preparing windows | Query evaluator over a captured model | Database page loading or mutation |
| Canonical edits and model publication | Existing serialized mutation/transaction boundary | Ad hoc projection patches by each caller |
| Selection, layout, transitions and media activation | Renderer | A second full-library metadata model |
| Media bytes, derivatives and detailed metadata | Existing media/detail services | Mandatory loading during library-model construction |

### What stays resident

Keep the data needed to identify, filter, order, count, and draw every visible root, including cover identity, relevant dimensions, type, name, palette/primary-color data, dates, rating, collection counts, sizes, memberships, and the fields searched by normal library queries.

Keep collection membership/order and root ownership where queries and mutations need them. Do not populate a million complete detail-view DTOs or retain a million decoded images.

Use the existing ID-keyed/shared structures where appropriate. A sparse root ID must not cause allocation proportional to the highest ID. Share strings and immutable values where this removes substantial duplication; do not introduce an elaborate global interning system merely to save a few bytes.

There is one logical value for each fact. Records and indexes may represent it differently for efficient lookup, but one owner derives and updates both. Indexes are accelerators, not separately writable models.

### What a grid read does

1. Capture one published snapshot.
2. Evaluate scope and filters using that snapshot.
3. Select the requested ordered window and the first four matches; compute exact aggregates.
4. Construct only the returned summaries and serialize the existing response shape.

Normal drawing-summary reads do not open SQLite transactions, acquire database reader permits, revalidate persisted pages, or repair derived data. No separate grid-loading API is introduced: keep `items.window` as the boundary.

UI preference/detail/persistence commands may still use SQLite. If a response combines those records with model data, its owner must deliberately provide consistency; do not accidentally remove transaction pairing from unrelated operations.

## 4. Search and ordering without a collection of fallback engines

All ordinary searchable text remains part of the resident model. Remove the rule that one exceptional row or one small fixed budget rejection switches the entire library onto a different browsing backend.

Use one text-matching contract: normalization, minimum input length, literal matching, and field boundaries are defined once. The same evaluator serves interactive filters and smart-folder evaluation. Exceptional strings are handled within that contract, not by rejecting every other row's resident data.

A straightforward resident scan is acceptable when its cost is acceptable. If broad searches need an index, use a resident literal-trigram candidate index followed by the same exact matcher, not another SQL fallback planner. Candidate production is an internal accelerator; it must not affect results. Do not add tiny/sparse/dense strategies by default. Each extra strategy must demonstrate enough benefit to justify maintaining it.

The implementation must choose and report its search strategy before declaring this slice done. It must not disguise a slow whole-library scan behind the phrase "in memory." If a dedicated search library is considered, first establish that it preserves our exact semantics and reduces total maintenance burden.

Preserve useful existing bitmap and direct-window ordering algorithms initially. Do not replace them with a full million-item sort for every request in the name of simplicity. Conversely, persisted tree nodes and their per-node recovery are not required merely because the in-memory tree is useful.

Result caching is optional acceleration over the same evaluator, not another source of truth. Prefer one small, understandable cache keyed by model revision and query. Retain more granular invalidation only where existing evidence shows a meaningful user benefit; simpler invalidation is acceptable even if it does slightly more work. Cache entries hold IDs/results and references, not private copies of library metadata.

## 5. Writes, snapshots and history

Reuse the existing serialized writer and immutable publication approach. Do not add a second event-sourced database, a generic dependency framework, or a library-wide mutable lock held throughout long queries.

The mutation sequence is:

1. Apply the requested canonical change in a SQLite transaction.
2. Gather affected roots and relationships through one mutation/settlement owner.
3. Prepare replacement resident records and affected index updates before commit. Re-read affected canonical records when that is simpler and safer than duplicating transformation logic.
4. Commit the database, then publish the already-prepared snapshot through the existing publication boundary.
5. Emit the committed revision/invalidation only after publication.

Failed preparation or commit discards the candidate snapshot. Successful publication must not depend on new fallible database work. A process failure between commit and publication is resolved by rebuilding from committed SQLite on the next open; no successful mutation acknowledgment is issued before publication.

An in-flight read may finish using its old immutable snapshot. It must not mix old and new fields. New reads capture the new snapshot. This is simpler than repeatedly asking whether a database result is "latest enough."

Share unchanged storage; replace affected records/chunks and index paths. Do not clone the entire library on each edit. Avoid premature custom allocation machinery: retain existing safe sharing unless a measured ownership problem requires more.

Imports use bounded batches and the same settlement owner. A somewhat slower batch is acceptable. Rebuilding the entire resident library for every batch is not. Background workers report canonical changes through this owner; no direct cache patching from thumbnail callbacks, repair jobs, or history code.

History retains canonical changes and affected identities, not old library snapshots. Undo/redo uses the same mutation route. Ensure collection creation, merge, split, cover replacement, and deletion update search, ordering, and drawing records together; this eliminates the recent winner-collection text asymmetry rather than adding another special repair step.

## 6. Opening, persistence and recovery

Prefer a simple readiness model: `closed -> loading -> ready`, with explicit failure/cancellation. Publish no partially resident browsing session. Show honest loading progress; accept that a large library may take longer to prepare.

The initial implementation should bulk-read canonical data from one consistent database snapshot and construct the resident model using a small number of streaming scans. Do not issue one query per root. Build necessary indexes, validate ownership, then publish the complete model.

Do not introduce first-window readiness plus background completion in the same slice. That creates partially ready states and fallback paths we are trying to remove. It can be considered later only if startup evidence warrants the extra state.

A persisted derived checkpoint is an optional startup optimization, not part of query correctness. Default to canonical bulk construction for the simplification slice. If retaining a checkpoint is necessary, it must use one clearly owned format/revision contract: load a valid snapshot or discard the derived checkpoint and rebuild. Avoid maintaining separate lazy admission and repair lifecycles for drawing, text, numeric, and alphabetical data.

Do not retain both the old independent persistence mechanisms and a new checkpoint indefinitely. Any temporary compatibility adapter needs an explicit removal milestone.

SQLite still owns durable FTS or other derived structures required by consumers outside browsing until those consumers are audited and migrated. "Queries use RAM" does not justify deleting tables that another feature still needs.

Schema cleanup follows the supported migration path with backup and transactional validation. Unknown canonical schemas remain untouched. A startup build failure must not destroy canonical data or silently open an incomplete library. Renderer reload reuses the existing backend session; it does not rebuild the model.

## 7. Memory is a measured tradeoff, not a guessed quota

Report separately: resident browsing data/indexes, retained older snapshots, query results and temporary construction memory, SQLite, renderer/Electron, and decoded-media memory. Include startup and import peaks, not only steady state.

Count shared allocations once and retain accounting until the last owner releases them. Do not claim that a million-item fixture proves a universal per-item memory cost: names, notes, URLs, tag density, collections, and text indexes vary substantially.

Remove the small all-or-nothing text-admission policy as a normal execution mode. This does not mean unlimited memory is safe. Validate allocation sizes, cancel obsolete readers, evict optional results first, and handle preparation failure explicitly. Establish a supported-size/memory-pressure policy from actual fixtures and host behavior before broad release; do not quietly reintroduce an entirely different SQL browsing engine or promise arbitrarily large libraries fit in RAM.

Neither "five GB" nor "one GB" is a new acceptance constant in this proposal. Choosing a modest slowdown to save a large amount of memory or complexity is valid; justify it in user terms.

## 8. Tests should protect intent, not fossilize mechanisms

Fewer tests is not itself the goal. Fewer invented contracts, redundant fixtures, and implementation-bound assertions is the goal. Correctness still matters in memory: stale snapshots, incorrect memberships, lost updates, and undo bugs do not disappear when SQL leaves the read path.

For each changed test, ask:

1. Which user requirement or engineering invariant does this protect?
2. Would a different correct implementation still pass?
3. Is the expected result independent of the code under test?
4. Is another test already covering the same contract?

### Keep and consolidate

- Compact examples of approved scope, search, ordering, preview, and collection behavior.
- Data-driven/property checks comparing optimized queries with a deliberately simple independent evaluator over a synthetic canonical fixture.
- A small set of end-to-end mutation/undo/reopen and publication/cancellation scenarios.
- Presentation tests for intentional transitions and coherent commits, without re-proving backend algorithms through extensive mocks.
- Focused regression cases for actual bugs, preferably added to an existing contract matrix rather than a new fixture stack.

The oracle must not call the candidate index or use its derived tables as its only truth. The earlier FTS oracle can share an index-corruption failure with the implementation; canonical fixture records and a simple reference matcher provide a more independent check.

### Rewrite or retire with the implementation

Tests for the old page cache's exact eviction order, helper-reader fan-out, cache admission thresholds, per-node persisted formats, or SQL call counts can leave when those mechanisms leave. Preserve the underlying requirement elsewhere if one remains.

Do not change expected user results just to make a rewrite pass. If a test appears to encode accidental behavior, flag the product question instead of treating either the test or the new code as automatically correct. Consolidation must not erase data-loss, security, migration, or cancellation coverage merely to reduce file count.

### Rules for agent-authored changes

- Implementation is not evidence of user intent. A new test must name its requirement or invariant in the change description; it need not create another tracking document.
- Prefer extending a canonical contract fixture to building a competing local model in mocks.
- No exact performance threshold in ordinary correctness tests. Benchmarks report distributions and tradeoffs separately.
- No production-only hook added merely to make an internal counter easy to assert, unless the hook has a clear diagnostic need.
- Architecture replacement and retirement of obsolete tests happen together. Tests are maintained code, not an untouchable archive.
- Do not add a new execution mode to preserve a test of an execution mode that the design intentionally removes.

## 9. Removal map

| Current mechanism | Target disposition |
| --- | --- |
| Drawing page cache, generation/checksum revalidation on reads | Replace with resident records in the published snapshot |
| Cold drawing helper threads and optional reader preopening | Remove from browsing; keep general database-pool safety fixes |
| Drawing-page queries and per-window canonical recovery | Remove from normal prepared-window execution |
| All-or-nothing text admission and SQL fallback dispatch | Replace with resident text and one matching contract |
| Independently persisted browsing domains/node repair | Retire after consumer audit/migration; optional single checkpoint only if justified |
| SQLite transaction/projection pairing for every grid window | Capture one model snapshot; retain pairing where details genuinely combine both stores |
| Multiple copies of summary facts across caches | Consolidate under one model owner |
| Query-result caches | Retain only useful, bounded acceleration; simplify invalidation where worthwhile |
| Existing membership/order indexes | Reuse first; simplify only when the replacement is both understandable and responsive |
| History snapshots and special projection patches | Canonical changes replayed through the shared settlement owner |

This is a deletion target, not a blanket authorization to remove similarly named tables or APIs. Check actual consumers before removal.

## 10. Bounded implementation sequence

1. **Agree on contracts and inventory ownership.** Use this document and the existing architecture page; do not create another collection of agent plans. Identify ambiguous behavior separately. Preserve current measurements and working-tree source as evidence, including untracked files.
2. **Complete the resident model.** Consolidate drawing records and searchable text into the existing snapshot owner. Bulk-build on open. Establish one exact matcher and working ordering path. Keep mutation changes scoped to affected records.
3. **Move normal browsing reads.** Keep the IPC shape and renderer behavior. Return one coherent prepared bundle from one captured model, without grid-data SQL reads. Move smart-folder evaluation to the same evaluator.
4. **Unify write ownership and remove superseded paths.** Route all mutation sources through the model owner; remove browsing helper reads, admission/fallback branches, obsolete persistence, and their implementation-only tests. Preserve supported library migration and non-browsing consumers.
5. **Review and measure once as a coherent slice.** Run an agreed, bounded verification set, report results and actual memory, and resolve material findings. Do not turn every observed fluctuation into another architecture branch or indefinite optimization campaign.

Temporary comparison code is acceptable in benchmark tooling, not as a permanent production feature flag selecting two complete backends. Do not maintain parallel normal browsing architectures after cutover.

## 11. Evidence and completion

The priority order is correctness and data safety, understandable ownership, responsive browsing, then startup/import/memory tradeoffs. A small regression may be acceptable if substantial complexity disappears. It should be visible in the report rather than hidden or automatically treated as a failed design.

Use the renderable million-item fixture and a metadata-heavy synthetic variant, including sparse IDs, Unicode, long text, collections, and a small nonempty tag-plus-text intersection. Equal totals and empty intersections alone do not establish candidate-aware performance coverage.

Measure normal opening/model construction, first and repeated prepared windows, meaningful smart-folder/text combinations, arbitrary jumps, refresh during mutations, retained-snapshot memory, and import throughput. Keep renderer timing and decoded-image memory separate. Use production builds, serial timing runs, fixed artifacts, and honest cold/warm labels. Reopening is not proof of an OS-cold cache.

The previous 50 ms number remains useful context, not a universal guarantee or justification for an expanding planner. Do not invent a new hard memory number or startup deadline from conversational estimates. Establish any release-blocking numeric limits with the user after the model's actual costs are visible.

This document launches no tests or benchmarks. Use existing reports where sufficient; agree on execution scope when implementation begins, and do not compete with measurements already running in another session.

Completion means:

- Normal prepared-grid queries need no SQLite reads for browsing records, matching, ordering, or aggregates.
- One owner settles and publishes browsing state; a rename/group/undo can be traced through that same path.
- The superseded loader/fallback machinery is removed, not merely bypassed by a larger cache.
- Tests protect approved semantics and essential ownership/resource invariants, with redundant mechanism tests retired.
- The implementation is assessed on representative latency, startup, import, and memory evidence, without unsupported speed claims.
- Remaining product questions and unsupported resource extremes are stated explicitly.
- The canonical architecture documentation is updated to describe the finished implementation; this proposal is then marked implemented, revised, or superseded.

## Recommendation

Proceed with a resident browsing model, using the existing immutable snapshot and serialized mutation foundations. Simplify the number of states and data owners before pursuing another small latency improvement.

The guiding question is not "can we make this branch faster?" It is "does this branch need to exist, and what user requirement would be lost if it did not?"

## Implementation status

Implemented as proposed: one `ProjectionSnapshot` holds drawing facts, settled text, names,
recently viewed order and folder parents alongside the existing bitmaps and indexes; `items.window`
and `items.query` run on a captured snapshot with no SQLite read; filters and smart folders share
one literal-substring contract with no admission budget, trigram probe or SQL fallback; one
publication owner settles the `browsing_dirty_root` journal before commit; the persisted drawing
pages, text chunks, name nodes, helper readers, page revalidation and per-domain repair lifecycles
are removed, and the supported predecessor schema migrates with a backup.

Deviations and open items:

- Duplicate inspection (`media_matches` scope) still resolves file identities from canonical
  tables, inside a consistent read that captures the snapshot it combines with. Making it
  resident would add a media-to-file map for one rarely used scope.
- The bitmap-and-index checkpoint is retained as the one optional persisted derived artifact; it
  is load-valid-or-rebuild and carries no journal or repair pass.
- The alphabetical name index is no longer persisted: the measured benefit was 25 to 40 ms of a
  roughly 530 ms load per million names, and the newest run was slower than the rebuild.
- Opening is cancellable end to end: `Library::open_with` takes a one-shot handle and reports
  stages, the backend runs the open on the blocking pool and cancels an in-flight open when a
  later open, a close or shutdown arrives, and the shell shows the stage in flight. Searching
  treats an embedded NUL as an ordinary byte; the old SQL predicate's NUL truncation was an
  artifact with no production reader and is not reproduced.
