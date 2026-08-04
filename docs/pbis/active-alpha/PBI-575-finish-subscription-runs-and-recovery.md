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

- A subscription can run on its saved daily, weekly, or monthly schedule and can also be triggered
  manually. Both triggers enqueue the same durable query jobs.
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
- durable query job leasing and startup reconciliation
- subscription downloads through the canonical ingest queue
- exact-hash duplicate reuse
- stop and reset commands exist
- indexed lookup of unresolved retry attempts across all history
- rebuilt subscription workspace with Health and History views

Release gaps:

- issue identity includes mutable message text
- SQLite uniqueness does not deduplicate subscription-wide issues whose `query_id` is null
- issue rows do not state a recovery action or next retry time
- not every failure path maps to one stable persisted issue
- Health displays issues but cannot consistently expose the correct recovery action
- the real create/run/fail/retry/import flow has not been proved in Electron

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

### S2. One failure classification path

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

### S3. Durable recovery semantics

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
- deleting or resetting an active subscription first cancels and settles its work

Deleting a subscription removes its definition and runtime history but never imported media. Resetting
preserves the definition and imported media, clears its run/download tracking and issues, and makes
the next run initial again.

Acceptance:

- retry state survives restart
- repeated retries do not duplicate jobs, attempts, or issues
- success resolves only the matching issue
- stop is idempotent and leaves no running task or leased job
- delete/reset cannot leave a worker, job, ingest row, or runtime task orphaned

### S4. Remove competing backend paths

Trace create, run, query run, retry, stop, reset, issue read, and ingest handoff from dispatch to
`SubscriptionRuntimeService`. Consolidate construction only where it removes duplicate behavior.
Delete `core/src/db/write/subscriptions.rs` if reachability confirms it is dead. Do not move the
service through the engine merely to satisfy the old PBI wording.

Acceptance:

- one production path exists for each behavior
- no subscription import bypasses canonical ingest
- no unused subscription command or persistence implementation remains
- command parity and focused tests stay green

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

"Retry all" is a backend operation over all eligible unresolved attempts, not the first 50 or 100
rows loaded for display. It returns a truthful attempted/queued/failed result, and the UI reports a
failure instead of silently discarding it. Health history is paginated; display limits never change
the action target or summary counts.

Acceptance:

- each backend recovery action has one visible, enabled UI action or an explicit no-action state
- resolved issues do not appear as active
- refreshing or reopening preserves the same Health state
- bulk retry cannot report success when every retry failed

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

Then run the real Electron workflow:

1. create a subscription and query
2. run it and import media
3. force or use a reproducible failure
4. confirm Health explanation and recovery action
5. retry or repair it successfully
6. restart and confirm settled history, media, and no open duplicate issue

Archive this PBI only after that smoke passes.

## Out of scope

- a new subscription architecture or service rename
- one PBI per supported site
- OnlyFans support or its future downloader library
- provider-specific import paths
- cloud syncing runtime history or credentials
- broad frontend refactoring
