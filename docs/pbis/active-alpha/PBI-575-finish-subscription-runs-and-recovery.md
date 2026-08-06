# PBI-575: Finish subscription runs and recovery

## Priority

P1 release blocker.

## Behavior contract

A subscription is a persistent definition containing one or more queries. Running it durably queues
those queries; each query either succeeds, is cancelled, or leaves a persisted problem that explains
what happened and what the user or worker can do next. Downloaded media enters the same durable
`add_media` ingest path as every other import, and exact duplicates settle as reuse. Closing and
reopening Picto must not lose failures, create duplicate problems, or leave a run permanently active.

`SubscriptionRuntimeService` is the current production boundary. Do not introduce another service
or rename it for architecture. Finish this path and delete only code proven to compete with it.

## Content and delivery contract

- A subscription owns one or more queries and a `manual`, `daily`, `weekly`, or `monthly` schedule.
  Running a subscription manually or from its schedule enqueues all enabled queries through the same
  durable job path.
- An individual query can be run manually for testing or catch-up, but queries never own schedules
  and are never scheduled independently.
- Groups organize subscriptions and may provide a manual run-all action; groups do not own recurring
  schedules.
- A run streams completed downloads into durable ingest while later posts are still downloading.
  It must not download the entire result set before ingestion begins.
- A multi-image post becomes one image collection when automatic collections are enabled. The
  collection is committed only when that post's expected members are ready; unrelated posts continue
  downloading and ingesting concurrently.
- When automatic collections are disabled, each downloaded image becomes an independent media
  entity and receives the post's source URLs, timestamps, tags, and other supported metadata.
- Post metadata belongs to the child images. Collection metadata is the aggregate of those children.
- Runtime progress must explain downloading, queued-for-ingest, ingesting, imported, reused, failed,
  retrying, blocked, cancelled, and complete states without waiting for the whole run to finish.
- Every source advertised by Picto must pass deterministic adapter/metadata tests and a real
  credential-backed integration run before release. Credentials remain local and are never fixtures.

OnlyFans is not a supported source in this phase. It may use a separately maintained downloader
library later; do not distort the current gallery-dl boundary to anticipate it.

## Current state

Already working and retained:

- persistent definitions, runs, query runs, jobs, download attempts, and progress
- durable query job leasing and restart continuation
- subscription downloads through the canonical ingest queue
- exact-hash duplicate reuse
- atomic stop, reset, and definition-only delete behavior
- indexed lookup of unresolved retry attempts across all history
- rebuilt subscription workspace with Health and History views

Release gaps:

- pending collection flushing assumes post assets are contiguous and can strand interleaved posts
- parsed rating metadata is not carried through canonical ingest
- the source picker advertises sites without source-specific metadata validation or live proof
- Python and Rust both participate in metadata normalization instead of having one clear owner
- removed source ids remain in active bridge, adapter, policy, or autocomplete code
- the real create/run/fail/retry/import flow has not been proved in Electron

## Phase execution rules

This phase is executed in order. Do not mix source-specific fixes into runtime correctness, do not
change the existing subscription UI structure, and do not resume general performance cleanup until
this PBI is archived.

1. Finish failure classification and durable recovery.
2. Finish streaming post assembly, metadata handoff, and run-scoped accounting.
3. Connect the existing progress and Health UI to that persisted truth.
4. Certify sources in bounded batches and expose only sources that pass.
5. Run one complete Electron workflow and archive the PBI.

Each implementation slice must remove any production path it replaces, pass focused Rust tests, pass
`cargo check`, and leave `git diff --check` clean before the next slice begins. A full alpha gate and
manual application verification are required at the end of each execution wave, not after every
small internal edit.

## Runtime boundary

The intended production flow is deliberately small:

`manual/scheduled subscription trigger -> durable query jobs -> gallery-dl item streams -> post assembler -> add_media queue -> ingest results -> persisted run progress`

- The trigger decides when work starts; it does not perform downloads itself.
- The source adapter emits normalized media items and explicit post completion information.
- The post assembler commits a complete multi-image post as one collection, or commits each image
  independently when automatic collections are disabled.
- The canonical ingest queue owns database insertion, duplicate reuse, and derivative scheduling.
- Persisted run/query/ingest state owns progress and recovery. In-memory task state is only a live
  projection and must be reconstructible after restart.
- A run is complete only when all query work and all ingest rows created by that run are terminal.

## Phase tickets

### S1. Stable issue identity and schema (complete)

Give each current problem one stable key independent of display text:

- query issue: `query:{query_id}:{issue_kind}`
- subscription issue: `subscription:{subscription_id}:{issue_kind}`

Store the key as non-null unique truth. Keep message and detail mutable as the latest evidence. Add
only fields required by behavior: `recovery_action` and optional `next_retry_at`. Do not add an
issue-event table unless a concrete user behavior needs issue history.

Acceptance:

- changing an error message updates one issue instead of inserting another
- repeated null-query issues remain one row
- first-seen is stable; last-seen and evidence advance
- current schema opens; any previous schema version is rejected without mutation

### S2. One failure classification path (complete)

Map gallery-dl, validation, ingest, and runtime failures through one typed disposition:

- `fix_credentials`: unauthorized or expired credentials
- `retry_automatically`: rate limit or transient network failure
- `retry_now`: retryable download or ingest failure
- `review_query`: extractor drift, metadata mismatch, or invalid definition
- `none`: terminal environment failure requiring a new build
- cancellation is a run outcome, not an issue

Persist the classified issue with a bounded log excerpt. Do not let orchestrator, executor, and sync
code invent separate status vocabularies.

Acceptance:

- every listed class produces the expected issue kind and recovery action
- debug noise does not change classification
- cancellation produces no open failure issue

Implementation checkpoint:

- First add the typed disposition and focused classification tests.
- Then route gallery-dl, validation, ingest enqueue, executor, and startup reconciliation failures
  through it one boundary at a time.
- Remove old message-based classification only after all call sites use the typed path.

### S3. Durable recovery semantics (implemented; live gate pending)

The database is the runtime authority: a full run owns enabled query jobs, the existing job queue
owns bounded transient retry timing, and query-only work has no full-run identity. Memory owns only
cancellation handles and live event projection. Groups own neither schedules nor pause state.

Automated proof covers schedule ownership, query-only runs, same-run restart recovery, bounded
backoff persistence, run-scoped multi-query finalization, idempotent Stop, group deletion, and media
retention across reset/delete. Before marking S3 complete, verify a live Stop, quit/reopen during a
run, and one transient automatic retry in Electron using a fresh schema-108 test library.

Live progress on 2026-08-05: Gelbooru ingestion was observed in Electron, and rebuilding/reopening
Picto re-claimed the same persisted run and query job without creating a replacement. Live Stop and
transient-retry verification remain before S3 closes.

Make the stored recovery action truthful:

- automatic retries use persisted `next_retry_at` and bounded backoff
- manual failed-post retry targets the exact unresolved attempt
- credential issues remain blocked until credentials are repaired
- review issues remain visible until the query is changed or a later success resolves them
- a successful matching run resolves the existing transient issue
- startup reconciles abandoned jobs/runs without discarding unresolved issues
- a worker consumes due automatic retries and enqueues one deduplicated retry job
- startup requeues interrupted query work instead of silently forfeiting it
- a failed or panicked query finalizes its run only after every sibling query is terminal
- finalization is scoped to the current run so stale or overlapping work cannot settle another run
- library close and application shutdown cancel and await active subscription executors
- deleting or resetting an active subscription first cancels and settles its work
- scheduled due-time is subscription-owned; a manual query run does not postpone the subscription
- a manual full-subscription run counts as that subscription's latest run and resets its due time

Deleting a subscription removes its definition and runtime history but never imported media. Resetting
preserves the definition and imported media, clears its run/download tracking and issues, and makes
the next run initial again.

Acceptance:

- retry state survives restart
- repeated retries do not duplicate jobs, attempts, or issues
- success resolves only the matching issue
- stop is idempotent and leaves no running task or leased job
- delete/reset cannot leave a worker, job, ingest row, or runtime task orphaned
- closing the library leaves no gallery-dl process or detached subscription executor running
- daily, weekly, monthly, manual, and paused subscription scheduling have deterministic tests
- individual query runs never create or alter recurring schedule state

Implementation checkpoint:

- Persist retry scheduling and add one worker path for due retries.
- Make startup reconciliation and multi-query finalization agree on the same terminal-state rule.
- Scope finalization by run id and connect executor cancellation to worker shutdown.
- Move recurring schedule ownership from groups to subscriptions in the current pre-1.0 schema and
  active API; do not retain group-schedule compatibility fields.
- Derive subscription due-time from full-subscription runs, never individual query runs.
- Finish stop, reset, and delete against that rule; do not special-case them in the UI.
- Verify this wave manually by stopping a live run, restarting Picto during another run, and retrying
  one reproducible failure. Do not begin source certification until these settle correctly.

### S4. Finish the one streaming ingest path and remove competitors

Checkpoint completed on 2026-08-05: downloading and ingest are independent workers joined by durable
rows, not an executor wait loop. Query jobs own downloader statistics, ingest items own
imported/reused/failed outcomes, and one idempotent finalizer writes the terminal run snapshot after
both sides settle. Either worker may invoke it; only the first terminal transition publishes.
Restart recovery requeues ingest leases, settles durable terminal runs, then repairs interrupted query
leases. Item outcomes and their parent ingest queue commit in one SQLite transaction, and completed
ingest evidence remains until its run snapshot exists. A cursor advances only after the whole fetched
batch is durably enqueued; failed handoffs are removed from gallery-dl's archive for retry. Missing
queue input is a visible failure, not fake reuse. This changes only the current pre-1.0 schema; no
migration or compatibility path is added. Live reset/re-run and interrupted-run proof remain.

Trace create, run, query run, retry, stop, reset, issue read, and ingest handoff from dispatch to
`SubscriptionRuntimeService`. Consolidate construction only where it removes duplicate behavior.
Delete `core/src/db/write/subscriptions.rs` if reachability confirms it is dead. Do not move the
service through the engine merely to satisfy the old PBI wording.

The same path must:

- count every downloaded single and collection member correctly
- carry rating and all other supported post metadata through canonical ingest and exact-hash reuse
- flush every complete post even if gallery-dl interleaves assets from different posts
- leave incomplete posts retryable instead of silently dropping or partially archiving them

Acceptance:

- one production path exists for each behavior
- no subscription import bypasses canonical ingest
- no unused subscription command or persistence implementation remains
- collection and non-collection modes preserve the same child metadata
- interleaved posts cannot leave completed collections pending in memory
- command parity and focused tests stay green

Implementation checkpoint:

- Group streamed assets by normalized post identity. Queue a post when its advertised child count is
  complete, or at source EOF when no count exists; never infer completion from contiguous ordering.
- Queue ready singles and complete posts immediately while the downloader continues.
- Require every subscription queue path to populate its existing `query_run_id`, and count ingest
  rows through that origin so progress and restart recovery account for exactly this run.
- Use one metadata builder for collection and non-collection modes. Rating, source URLs, timestamps,
  tags, title, and notes must reach child entities through the same ingest path.
- Keep the gallery-dl bridge responsible for transport and raw extractor events; normalized Picto
  metadata has one Rust owner before canonical ingest.
- Delete a competing subscription-only ingest or metadata path in the same change that replaces it.
- Verify this wave manually with one single-image post, one multi-image post in each collection mode,
  and enough results to observe download and ingest overlapping.

### S5. Truthful Health actions

The Health view must show the persisted classification in plain language and expose exactly its
recovery action:

- fix credentials opens Accounts for the relevant site
- retry now retries the exact failed post or query
- automatic retry shows when it will run and does not offer a misleading button
- review query opens query editing
- terminal environment failure explains that retrying will not help
- reset is available where it is a valid recovery action

The frontend does not infer recovery from message text.

The existing subscription workspace remains. This ticket changes only progress truth, recovery
actions, pagination, and error reporting; it is not a UI rewrite.

Progress is scoped to the current run and stays visible until all of that run's ingest rows are
imported, reused, or failed. Historical queue rows never inflate current progress, and finishing
downloads does not imply ingestion is complete.

Backend checkpoint on 2026-08-06: query runs now persist gallery-dl's outcome as an internal
settling state while their public status remains running. A query remains active across restart
while any ingest row with its `query_run_id` is pending or running; the executor and ingest worker
share one idempotent settlement path, and an ingest failure overrides an otherwise successful source
result. This uses the existing schema and adds no migration or compatibility path.

"Retry all" is a backend operation over all eligible unresolved attempts, not the first 50 or 100
rows loaded for display. It returns a truthful attempted/queued/failed result, and the UI reports a
failure instead of silently discarding it. Health history is paginated; display limits never change
the action target or summary counts.

Acceptance:

- each backend recovery action has one visible, enabled UI action or an explicit no-action state
- Run actions are unavailable whenever durable runtime state says the subscription is already active
- backend failures are shown as plain product messages without Electron IPC transport prefixes
- resolved issues do not appear as active
- refreshing or reopening preserves the same Health state
- bulk retry cannot report success when every retry failed
- download and ingest counters remain truthful while both stages overlap
- accepted media from an interrupted segment remains included in query totals after the run resumes

Implementation checkpoint:

- Derive every counter from persisted rows scoped by run id; do not patch historical subscription
  totals into a current progress event.
- Keep the current progress card until the run's last ingest row is terminal, then retain the settled
  result in History.
- Bind Health buttons directly to `recovery_action` and make backend bulk operations independent of
  UI pagination.
- Verify this wave manually after S4: watch counters advance during a run, reload the subscriptions
  screen, restart Picto, and confirm the same active or settled state returns.

### S6. Release verification and closure

Keep unit tests at classification and persistence boundaries. Add end-to-end backend tests for
queueing, restart reconciliation, issue deduplication, retry, success resolution, and canonical
ingest. Maintain a source verification matrix listing every advertised source and query kind, its
credential owner, deterministic fixture coverage, live verification date, and known limitation.

For every advertised source, a credential-backed run must prove:

- query construction and pagination/resume behavior
- canonical post and media URLs
- source timestamps and supported tags/metadata
- single-image ingest
- multi-image collection ingest with child metadata
- non-collection ingest with metadata copied to every image
- visible progress while download and ingest overlap
- rerun deduplication and restart recovery

The source picker is an allowlist of passed sources, not a gallery-dl capability list. Any source
without deterministic metadata validation and a recorded live pass stays hidden until it earns
support. Contradictory credential declarations must be fixed or the source removed from the allowlist.

Then run the real Electron workflow:

1. create a subscription and query
2. run it and import media
3. force or use a reproducible failure
4. confirm Health explanation and recovery action
5. retry or repair it successfully
6. restart and confirm settled history, media, and no open duplicate issue

Archive this PBI only after that smoke passes.

## Source certification waves

Do not attempt all sources at once. The registry entry for a source is not permission to advertise
it. Certification proceeds in small batches:

1. Prove the shared harness with one public booru and one authenticated source.
2. Certify the strongest initial candidates: Gelbooru, Danbooru, Pixiv, e621, and FurAffinity.
3. Certify remaining sources individually; hide any source that lacks credentials, deterministic
   metadata expectations, or a successful live run.

For each source, record the tested query, expected post identity, expected child count, expected
source URLs, timestamps, tags/rating, authentication mode, collection behavior, rerun result, and
verification date. Store no secrets or downloaded user content in Git.

User verification is requested only when a batch is mechanically ready. The user supplies local
credentials and expected examples, runs or observes the live workflow, and accepts the visible
result. A failed certification creates a bounded source fix inside S6; it does not trigger a new
subscription architecture.

The certification runner is strict. Missing credentials, placeholder queries, rate limits, network
failures, and inconclusive probes do not pass a source. Remove stale production references for source
ids no longer registered. OnlyFans-like content reached through a third-party aggregator proves that
aggregator, not native OnlyFans support.

## Agent plan

- Coordinator: owns contract decisions, shared runtime/schema integration, review, verification, and
  commits.
- Runtime agent: S2 classification only.
- Recovery agent: S3 queue/reconciliation semantics only, after S2 lands.
- Streaming agent: S4 source event and post assembly only.
- Ingest agent: S4 metadata/origin propagation only, with a disjoint file set.
- Frontend agent: S5 existing progress and Health surfaces only, after backend contracts stabilize.
- Verification agents: S6 source fixtures and live-check reports in disjoint source batches.

Agents do not refactor adjacent code, create compatibility shims, stage, or commit. The coordinator
reviews every patch against this behavior contract and rejects changes that add a second path.

## Out of scope

- a new subscription architecture or service rename
- one PBI per supported site
- OnlyFans support or its future downloader library
- provider-specific import paths
- cloud syncing runtime history or credentials
- broad frontend refactoring
