# Picto Subscription Engine Rebuild

> Status: deferred design reference, not a `0.6.0-alpha` release gate. The replacement
> `gallery_job`/attempt schema and pull/ack engine described below are not implemented. The release
> therefore retains the working per-query gallery-dl archive and transient E-Hentai gallery path;
> delete them only in the same change that ships their replacement. The temporary schema converter
> and Picto-owned Webtoons support have been removed.

## Objective

Replace the current subscription and gallery-download orchestration with one small, durable,
post-serial engine built around Picto's actual behavior:

```text
discover one post
-> record traversed
-> download every usable file in that post
-> publish file progress
-> canonically ingest the complete post
-> verify the canonical result
-> record added or skipped
-> acknowledge settlement
-> discover the next post
```

This is a replacement of the orchestration path, not another compatibility layer around it.
`gallery-dl` remains the extractor/downloader implementation. SQLite remains Picto's authority.

## Why This Is Necessary

The current path represents the same lifecycle in too many places:

- The gallery-dl archive database.
- Python bridge events and acknowledgement state.
- `source_post` and `source_item` states.
- Subscription run and run-query states.
- The generic canonical ingest queue.
- Transient subscriptions used to represent gallery downloads.
- Renderer-side gallery completion and cleanup.

Those layers can settle independently. A current real failure proves the problem:

- E-Hentai gallery `1449482` downloaded 30 files.
- Ingest job `986` is marked `succeeded`.
- All 30 `source_item` rows say `ingested`.
- Every `source_item.media_item_id` is null.
- `source_post.root_item_id` is null.
- No source provenance or Inbox root exists.
- The renderer still announced success.

The immediate causes are also concrete:

- `query_ingest_settlement` treats the absence of `downloaded` rows as success instead of proving
  a terminal post outcome and live canonical root.
- Previously deleted media can leave tombstoned stable keys and orphaned source rows whose stale
  `ingested` state survives later attempts.
- `publish_source_progress` is a no-op and relies indirectly on unrelated invalidation behavior.
- A transient gallery is considered complete from run status before canonical output is verified.

Do not repair this by adding more reconciliation states. Remove duplicate ownership.

## Scope And Ownership

### The New Engine Owns

- Scheduled and manual subscription runs.
- Gallery import runs.
- Per-query source cursors.
- One exclusive active execution per subscription.
- One-at-a-time source-post traversal.
- Per-post download staging and progress.
- Canonical post ingestion.
- Added, skipped, and failed outcomes, with orthogonal warning records.
- Restart recovery and idempotency.
- Subscription reset and run history.
- Compact progress invalidation.

### `gallery-dl` Owns

- Provider extraction.
- Provider-specific URL construction and pagination.
- Network requests and media transfer.
- Provider metadata returned to Picto.

### Canonical Library Owns

- Root, collection, media-item, and media-file persistence.
- Physical content-hash deduplication.
- Source provenance.
- Tag/folder/lifecycle projection updates.
- Thumbnail readiness before publication.
- Exact-hash metadata transfer rules.

### Authentication Remains Separate

Do not modify authentication while implementing this plan.

- Picto-managed browser login and OS credential storage remain the only product auth path.
- Existing auth adapters provide normalized cookies, headers, or tokens to the source adapter.
- Do not add external Chromium, pasted credentials, or provider-specific auth workarounds.
- Do not edit `electron/windows/authSessions.mjs`, `electron/windows/authSites.mjs`, or related auth
  tests unless a separately approved auth task explicitly requires it.

## Canonical Runtime Model

### One Post State Machine

Persist exactly one mutable attempt state for the current run/query/post:

```text
discovered
-> downloading
-> downloaded
-> ingesting
-> added
```

Terminal alternatives:

```text
discovered/downloading -> skipped(reason)
discovered/downloading/ingesting -> failed(reason)
```

Terminal post outcomes are exactly `Added`, `Skipped(reason)`, or `Failed(reason)`. Warnings are
orthogonal records attached to an outcome, never an outcome themselves.

Rules:

- `traversed` increments when `discovered` commits.
- `files downloaded` increments after each staged file is durable and verified to exist.
- `added` commits only after canonical ingest returns and the engine verifies a live root and every
  expected source-to-media association.
- `skipped` means no usable media, an already-settled exact duplicate, or another explicit
  non-failure terminal reason. Skipped outcomes persist an explicit reason code.
- A successfully ingested post with one or more non-fatal media failures is `Added` plus warning
  records. Warnings must not prevent `posts_added` incrementing and must not cause the post limit
  to overrun. Problems retain the post link and concise reason.
- Media failure semantics are exhaustive: partial success is `Added` plus warnings; a post with no
  media descriptors is `Skipped(NoUsableMedia)`; a post whose media are all unavailable after
  bounded retries is `Skipped(AllMediaUnavailable)` plus problems; exhausted transient retries on
  required work or a canonical ingest failure is `Failed`.
- A failed post/run is reserved for a condition that prevented a valid terminal outcome.
- The next post cannot be requested before the current post reaches a terminal state.
- The configured post limit counts `added` only. Skips never consume it.
- Runs must still terminate: at provider exhaustion, or at a known previously settled frontier.
  For providers that cannot expose a finite frontier, apply a safety bound defined as N
  consecutive terminally settled posts with zero `Added` outcomes in the current run (default
  N = 10 × the configured post limit, persisted per run-query as a counter that resets on every
  `Added`). The bound is a **resumable traversal-budget stop**, not a failure and not success:
  hitting it settles the run-query with status `budget_exhausted` and terminal reason
  `safety_bound`, with the cursor persisted
  at the last settled post, so the next run resumes exactly there and keeps digging. It may
  legitimately fire mid-way through a freshly reset query's duplicate backfill — no work or
  frontier progress is lost, the follow-up run continues the walk. It must not advance the cursor
  past any unprocessed post, and a run ending in `safety_bound` is reported as such in the UI.

### One Active Execution Per Subscription

A subscription owns one exclusive execution lease. Starting work is an atomic backend operation,
not a renderer convention.

- A full-subscription run acquires the lease and may contain every eligible query, but executes
  those queries serially. Only one child `source_run_query` may be running at a time.
- A manually selected query acquires the same lease and creates a run containing exactly that
  query.
- While either form is active, no other full run or individual query from that subscription may be
  started or queued. A competing request returns the existing active run as a conflict.
- The lease remains held until the run completes or Stop durably cancels it. Pausing or putting a
  definition on hold does not permit a second execution to overlap the existing run.
- The UI disables every other Run action for that subscription while the lease is held, but SQLite
  is the authority that prevents races between UI actions, schedules, retries, and restart recovery.
- Gallery jobs are not subscription runs and use the shared scheduler without acquiring a
  subscription lease.

### Stop And Cancellation

Stop cancels at the attempt level, not just the run level:

- During `discovered`/`downloading`: the attempt settles as `cancelled` with reason `stopped`, the
  cursor does NOT advance past it (the same post is rediscovered next run), staged files for the
  attempt are deleted, and the Python worker receives a CANCEL for the open attempt — it must
  acknowledge before the run releases the subscription lease.
- During `ingesting` (canonical commit already issued): cancellation waits for the commit, then
  settles the attempt normally (`added`/`skipped`) — a committed ingest is never abandoned — and
  only subsequent attempts are cancelled.
- Restart with an open `cancelled`-pending attempt: recovery settles it as `cancelled` using the
  crash-recovery provenance check first, so a crash during Stop still settles exactly once.
- A stale Python process that keeps emitting events for a cancelled attempt is fenced by the
  protocol correlation identifiers; its events are dropped.
- Cancellation acknowledgement is bounded: if the Python worker does not ACK within 10 seconds,
  the leased worker process is terminated (SIGKILL), the open attempt settles as `cancelled`
  through the recovery path, and the lease releases. Stop can therefore never hang on a stuck
  extractor; the same bound applies during restart recovery.
- An issued canonical ingest transaction always finishes — it is local Rust/SQLite work with
  bounded lock acquisition (busy_timeout), and nothing can or should abort it mid-commit. Stop
  therefore returns "cancellation requested" immediately; the in-flight post's settlement
  completes asynchronously, and the lease releases once that settlement commits. Only the
  extractor side has a kill path.
- Forced termination never takes down unrelated work, because the worker topology is one
  bounded reusable pool whose members are exclusively leased to a single attempt at a time —
  never a process spawned per execution, never one killable worker shared concurrently. Killing
  a leased member affects exactly its own attempt.

### Crash-Safe Ingest Boundary

The exact settlement sequence per post is:

```text
canonical ingest commit
-> verify canonical root/provenance
-> commit attempt outcome and cursor
-> publish progress
-> ACK Python
```

- Never ACK or advance the cursor before the attempt outcome commits.
- A crash between canonical commit and attempt settlement must recover through source provenance
  and settle exactly once, without creating another root.

### Minimal Durable Tables

Keep stable definitions and history, but eliminate parallel state ownership.

Retain or replace with direct schema-1 equivalents:

- `subscription`: definition and schedule.
- `subscription_query`: provider query, cursor, grouping choice, pause state.
- `source_run`: one manual or scheduled execution.
- `source_run_query`: one query execution and aggregate terminal status.
- `source_post`: stable provider post identity and normalized metadata only. No mutable state.
- `source_item`: stable provider media identity and metadata only. No `state` column, and no
  duplicated canonical media ownership — `source_provenance` already owns the source-to-media
  association.
- `source_post_attempt`: the sole persisted post state for its execution owner. Created roots are
  recorded as attempt-to-root result rows, not a singular column: disabling "Group multi-media
  posts" legitimately creates multiple standalone roots from one post. `Added` requires one or
  more live result roots; only gallery imports require exactly one collection root. An exact
  duplicate is `SkippedExactDuplicate` and may reference the matched provenance without pretending
  it created a root.
- `source_file_attempt`: staged path, bytes, and download outcome for current-post progress.
- `subscription_issue`: user-visible warnings/problems.
- `gallery_job`: gallery-import identity and presentation metadata only, with a foreign key to its
  run/attempt (see Gallery Imports).

### Concrete Schema Requirements

This is the durable model. The implementing agent refines names, not semantics.

```sql
CREATE TABLE source_run (
    run_id INTEGER PRIMARY KEY,
    subscription_id INTEGER REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    requested_by TEXT NOT NULL CHECK (requested_by IN ('manual','manual-query','schedule','gallery')),
    status TEXT NOT NULL CHECK (status IN
        ('pending','running','succeeded','budget_exhausted','failed','cancelled')),
    created_at TEXT NOT NULL,
    finished_at TEXT,
    -- Gallery runs carry no subscription; subscription runs always do.
    CHECK ((requested_by = 'gallery') = (subscription_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX idx_source_run_active_subscription ON source_run(subscription_id)
    WHERE subscription_id IS NOT NULL AND status IN ('pending','running');

CREATE TABLE source_run_query (
    run_query_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES source_run(run_id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('query','gallery')),
    query_id INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN
        ('pending','running','succeeded','budget_exhausted','failed','cancelled')),
    terminal_reason TEXT,          -- e.g. 'budget_met','exhausted','frontier','safety_bound',...
    available_at TEXT NOT NULL,
    -- SourceOwner union enforced: query-owned rows carry query_id, gallery
    -- rows carry none (their gallery_job references them; see trigger below).
    CHECK ((owner_kind = 'query') = (query_id IS NOT NULL)),
    UNIQUE(run_id, query_id)
) STRICT;
CREATE UNIQUE INDEX idx_source_run_one_running_query ON source_run_query(run_id)
    WHERE status = 'running';
-- UNIQUE(run_id, query_id) does not constrain NULL query_id rows: one gallery
-- execution per run is enforced separately.
CREATE UNIQUE INDEX idx_one_gallery_execution_per_run ON source_run_query(run_id)
    WHERE owner_kind = 'gallery';

-- Execution identity is immutable after insertion; every other invariant on
-- these rows (owner coherence, born-pending states, gallery-job presence)
-- lives in the single typed Rust transition path and its contract tests,
-- not in defensive SQL.
CREATE TRIGGER run_query_identity_immutable
BEFORE UPDATE OF run_id, owner_kind, query_id ON source_run_query
BEGIN SELECT RAISE(ABORT, 'execution identity is immutable'); END;

CREATE TABLE source_post_attempt (
    attempt_id INTEGER PRIMARY KEY,
    run_query_id INTEGER NOT NULL REFERENCES source_run_query(run_query_id) ON DELETE CASCADE,
    source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN
        ('discovered','downloading','downloaded','ingesting','added','skipped','failed','cancelled')),
    terminal_reason TEXT,          -- required for skipped/failed via trigger or app invariant
    cursor_scope TEXT,             -- provider stream partition this post belongs to
    boundary_cursor TEXT,          -- cursor value committed with the terminal outcome
    started_at TEXT NOT NULL,
    settled_at TEXT,
    UNIQUE(run_query_id, source_post_id)
) STRICT;
-- one non-terminal attempt per execution owner:
CREATE UNIQUE INDEX idx_attempt_open ON source_post_attempt(run_query_id)
    WHERE state NOT IN ('added','skipped','failed','cancelled');

CREATE TABLE source_attempt_root (   -- attempt-to-root results; >=1 row required for 'added'
    attempt_id INTEGER NOT NULL REFERENCES source_post_attempt(attempt_id) ON DELETE CASCADE,
    root_id INTEGER REFERENCES library_root(root_id) ON DELETE SET NULL,
    root_stable_key TEXT NOT NULL,   -- snapshot at settlement; history survives
                                     -- permanent deletion of the root itself
    PRIMARY KEY(attempt_id, root_stable_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE source_file_attempt (
    file_attempt_id INTEGER PRIMARY KEY,
    attempt_id INTEGER NOT NULL REFERENCES source_post_attempt(attempt_id) ON DELETE CASCADE,
    item_key TEXT NOT NULL,
    staged_path TEXT,
    bytes_total INTEGER,
    bytes_staged INTEGER NOT NULL DEFAULT 0,
    outcome TEXT CHECK (outcome IN ('staged','skipped','failed')),
    error TEXT,
    UNIQUE(attempt_id, item_key)
) STRICT;

CREATE TABLE gallery_job (
    gallery_job_id INTEGER PRIMARY KEY,
    run_query_id INTEGER NOT NULL UNIQUE
        REFERENCES source_run_query(run_query_id) ON DELETE RESTRICT,
    service TEXT NOT NULL,
    url TEXT NOT NULL,
    expected_media_total INTEGER,
    created_at TEXT NOT NULL,
    dismissed_at TEXT
) STRICT;

-- The two enforced-value invariants SQLite owns: Added carries live roots,
-- and a gallery settle is exactly one collection root. Reason codes,
-- timestamps, and state ordering are the Rust transition path's contract.
CREATE TRIGGER attempt_added_requires_live_roots
BEFORE UPDATE OF state ON source_post_attempt
WHEN NEW.state = 'added' AND NOT EXISTS
    (SELECT 1 FROM source_attempt_root r
     WHERE r.attempt_id = NEW.attempt_id AND r.root_id IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'added attempt requires live result roots'); END;

CREATE TRIGGER gallery_added_requires_one_collection_root
BEFORE UPDATE OF state ON source_post_attempt
WHEN NEW.state = 'added'
    AND EXISTS (SELECT 1 FROM gallery_job g WHERE g.run_query_id = NEW.run_query_id)
    AND ((SELECT count(*) FROM source_attempt_root r WHERE r.attempt_id = NEW.attempt_id) != 1
         OR (SELECT count(*) FROM source_attempt_root r
             JOIN library_item item ON item.local_id = r.root_id
             WHERE r.attempt_id = NEW.attempt_id AND item.item_kind = 2) != 1)
BEGIN SELECT RAISE(ABORT, 'gallery import requires exactly one collection root and nothing else'); END;
```

Query cursors are per stream partition: `subscription_query_cursor(query_id, cursor_scope,
cursor_value, updated_at, PRIMARY KEY(query_id, cursor_scope))`. Single-stream providers use one
scope (`'feed'`). Settlement updates only the current scope, atomically with the attempt outcome.
Run state, counters, phase, downloaded counts, and warnings live in the attempt tables for both
owners; `gallery_job` holds identity and presentation only and **references its execution**
(`gallery_job.run_query_id`), matching one cascade graph. Dismissal is one explicit Rust
transaction: refuse while the execution is active (Stop first), then delete the job and its
`source_run` (whose `source_run_query` rows cascade) together. That path and its contract test —
not defensive SQL — own the no-orphan guarantee.

Do not use a gallery-dl archive database as a second history authority. Picto's source identities,
provenance, and post outcomes already provide the required idempotency.

Do not route subscription posts through a second durable `ingest_job` lifecycle. Once a post is
fully downloaded, the source worker invokes the canonical library ingest API directly and stores
the result in the same post-attempt transition. The generic ingest queue may remain for unrelated
manual/background imports if those operations still need it.

### State Invariants

Enforce these in code and tests:

- `Added` requires at least one live `source_attempt_root` row; gallery imports require exactly
  one, referencing a collection root.
- Every successfully downloaded usable source item in an added post has live provenance in
  `source_provenance`.
- Source tables carry no mutable ingest state; only attempt rows do.
- A post cannot be both added and skipped.
- A run cannot succeed while any started post is non-terminal.
- A subscription cannot have more than one active run, and a full run cannot have more than one
  running child query.
- A query cannot have more than one non-terminal post attempt.
- A later post cannot have a download attempt while an earlier post is non-terminal.
- Progress counts derive only from attempt rows, never from inferred combinations of unrelated
  tables.
- Renderer cleanup cannot alter or manufacture backend success.

## gallery-dl Integration

### Use The Library, Not CLI Orchestration

The extractor topology is one bounded reusable pool of Python worker processes, driven by the
persisted Rust worker, importing the vendored `gallery_dl` package directly. A pool member is
exclusively leased to one attempt at a time (so cancellation can kill it without collateral) and
returns to the pool at settlement. Do not create an uncontrolled process per query and do not
share one member across concurrent attempts. Every request acquires a token from one shared
per-domain limiter.
Cancellation, fairness, and retry/backoff are defined centrally in the Rust worker; cursor
advancement and retries cannot belong to provider adapters. The Python process is an extractor
service, not a durable worker and not an authority.

All provider-side state — download-history databases, caches, incremental markers — lives under
the engine's per-query state directory and is engine-owned. No provider tool may consult global or
cross-query state: whether a post was already downloaded is a per-query question answered by
Picto's attempt history, never by a provider tool's own ledger.

Use a pull/ack protocol:

```text
Rust -> NEXT_POST(cursor)
Python -> POST(metadata, stable identity, expected media descriptors)
Rust commits discovered/traversed
Rust -> DOWNLOAD_CURRENT_POST
Python -> FILE_STAGED for each completed file
Python -> CURRENT_POST_DOWNLOAD_COMPLETE
Rust commits download progress and canonical ingest
Rust verifies root/media/provenance
Rust commits the attempt outcome and cursor
Rust -> ACK_POST(outcome, warnings)
Rust -> NEXT_POST(next cursor)
```

Every protocol command and event carries `run_query_id` and `attempt_id`; the worker rejects any
event whose identifiers do not match the currently open attempt (stale events from a cancelled or
superseded execution are dropped and logged, never applied). `ACK_POST` carries the terminal
outcome (`Added`, `Skipped(reason)`, `Failed(reason)`) plus any warning records — warnings are
never an outcome. The Python iterator must not be advanced after
the current post boundary until `ACK_POST` arrives. If a gallery-dl extractor internally fetches
the next post before yielding the current boundary, adapt that provider inside its gallery-dl site
adapter by using a bounded one-post extractor window. Do not weaken the global engine or add
local-provider shims.

Prove the pull behavior before schema implementation: first prototype `NEXT_POST`,
`DOWNLOAD_CURRENT_POST`, and `ACK_POST` against representative provider classes, and identify
which gallery-dl APIs or hooks expose post boundaries and media descriptors. Do not assume every
extractor exposes a generic post-list API.

### Request Pacing

- Apply provider-specific randomized pacing at the actual HTTP request boundary, using a 0.5-2 second fallback.
- Do not add a second unconditional sleep after extraction events, file writes, acknowledgements,
  ingest, or UI publication.
- Permit bounded concurrent media downloads only within the current post.
- Never prefetch a later post.
- Record request start/end and engine-state timestamps in debug traces so avoidable gaps are
  measurable.

### Provider Adapter Boundary

Each provider adapter may define only:

- Query normalization.
- gallery-dl extractor category/subcategory.
- Authentication material mapping.
- Cursor/page-size settings needed to enforce one-post delivery.
- Source URL normalization.
- Canonical metadata and tag normalization.

Adapters must not own run counters, persistence, retry loops, ingest, publication, or UI behavior.

## Canonical Ingest And Duplicate Rules

### Standalone Incoming Post

- A new physical hash creates one standalone root in Inbox.
- An exact physical hash creates no new visible root.
- Exact duplicates attach incoming source provenance to the retained physical media.
- Incoming standalone tags are unioned onto every existing root that owns that exact media,
  including Active, Inbox, Trash, and collections.
- Existing collection tags are never copied back to an incoming standalone item.
- If no new root is created, the source post is `skipped`, not `added`, even when metadata transfer
  changed existing roots.

The completed exact-hash fanout change already present in `picto-library/src/ingest.rs` and
`picto-library/tests/greenfield_contract.rs` should be preserved and reviewed, not reimplemented.

### One Canonical Ingest Implementation

Subscription attempts and ordinary `ingest_job` work may have different durable envelopes, but
they must call the same canonical prepared-ingest transaction implementation. Do not create
separate subscription-specific deduplication, thumbnail, tag, collection, or publication logic.

### Incoming Collection Post

- Store each physical file once by content hash.
- Preserve a structurally new collection even when one or more members reuse physical files.
- Collection tags remain on the incoming collection root.
- Do not transfer an incoming collection's tags to existing standalone roots or collections.
- Do not transfer an existing collection's tags to the incoming collection.
- An existing source-identical collection is skipped idempotently.
- A new collection publishes only after its cover thumbnail exists and the complete member vector
  is canonical.

### Tombstones And Explicit Gallery Imports

- Automatic subscriptions respect permanent-deletion tombstones and skip re-import.
- An explicit Add Gallery action is a deliberate user request and may revive matching tombstoned
  source stable keys inside the canonical ingest transaction.
- Tombstone removal and canonical root creation must be atomic.
- A failed explicit gallery ingest must not report success or leave source rows as ingested.

## Gallery Imports

Gallery imports use the same post processor but are not represented as transient subscriptions.

- `source_post_attempt` remains the sole run-state and counter authority for gallery imports.
- `gallery_job` stores only gallery-specific identity and presentation metadata (URL, service,
  expected image total, timestamps), with a foreign key to its run/attempt. Do not duplicate
  phase, downloaded count, warning state, or root ownership in both tables.
- Progress ownership is expressed as one owner type instead of mandatory `run_id`/`query_id`:

```text
SourceOwner =
  SubscriptionQuery { run_id, query_id }
  | GalleryJob { gallery_job_id }
```
- E-Hentai/ExHentai gallery metadata describes one source post with N media items.
- The UI initially shows `0 images downloaded`.
- Once a stable total is known, it shows `downloaded / total images downloaded` without reverting
  to a provisional or count-only label.
- Each durable file completion invalidates gallery progress immediately, coalesced to at most ten
  publications per second.
- Nothing appears in Inbox until the complete collection and cover thumbnail are ready.
- Success requires exactly one live canonical root ID.
- The success notification is emitted from the backend terminal result, not inferred from a
  disappearing transient definition.
- Failed gallery jobs remain inspectable and retryable until the user dismisses them.

Delete the renderer-owned `settleFinishedGalleryImports` cleanup behavior after the new backend
job API is connected.

## Subscription Reset

Reset operates only on the selected subscription and its queries.

It must:

- Require or perform a clean stop before resetting.
- Clear run history, counters, cursors, skipped state, warnings, errors, current attempts, staged
  files, and provider download history for that subscription/query.
- Leave authentication intact.
- Leave canonical roots already added to the library intact.
- Leave every other subscription's history untouched, even if it uses the same provider and query.
- Make the next manual run begin from a clean provider cursor.

Because the new engine has no gallery-dl archive authority, reset is a single SQLite-owned cleanup
plus staged-file deletion.

## Retry And Cleanup

Failure classes are exhaustive and centrally owned (never inside provider adapters):

- 401, and 403 from extractor/API/login endpoints: permanent for the run — the query fails with
  kind `unauthorized` and credential health is flagged.
- 403 from a media/CDN URL (expired or forbidden signed link): permanent for that file only —
  file `failed` with a warning; the post settles per media-failure semantics. Media-level
  forbidden responses never fail the query.
- 404/410: permanent for that file — file `failed`, post gains a warning; if every file is
  permanent-unavailable the post is `Skipped(AllMediaUnavailable)`.
- 429: park the query with kind `rate_limited`, honoring `Retry-After` (or the provider's stated
  reset) plus a 2-minute buffer; no file-level retry loop.
- 408, 5xx, connection reset, DNS, timeout: transient — retry the same file up to 3 times with
  exponential backoff (2s, 4s, 8s) inside the current attempt.
- Integrity failure (size/hash mismatch, truncated or undecodable file): one re-download, then
  file `failed` with a warning.
- Canonical ingest failure: post `Failed(reason)` with no retry — it is a bug, not weather.
- Escalation: a post fails only when required work exhausts retries; a query fails after 3
  consecutive post failures; the run aggregates query outcomes.
- Preserve staged files only while they can be reused safely (resuming the same post attempt).
- Delete staging after `Added`, `Skipped`, reset, cancellation, or dismissal.
- Failed gallery jobs remain inspectable but have a bounded retention policy.

## Progress And UI Contract

Expose one backend progress DTO derived from durable attempt state:

```text
SourceRunProgress {
  owner,          // SourceOwner: SubscriptionQuery { run_id, query_id } | GalleryJob { gallery_job_id }
  current_post_key,
  phase,
  posts_traversed,
  posts_added,
  posts_skipped,
  files_downloaded,
  current_post_files_downloaded,
  current_post_files_total,
  gallery_files_total,
  warning_count
}
```

Rules:

- The backend also exposes one aggregate read model per subscription (sums of the active run's
  child run-query attempt counters plus the serial position, e.g. "query 3 of 9") — subscription
  cards consume that aggregate; the renderer never merges per-query events itself.
- Persist first, then publish.
- Publish each file completion, coalesced only when multiple completions occur inside 100 ms.
- UI never increments counters speculatively.
- Notifications report roots/posts added, not raw media-file count.
- Problems link to the source post and show one concise failure reason; do not create a second row
  for the failed media URL.
- Reset clears all visible progress and failure text for the reset owner.

## Implementation Sequence

### 1. Freeze Behavior With A Fake Provider

- Add a deterministic source adapter that yields multi-file, no-media, exact-duplicate, warning,
  and source-exhaustion posts.
- Write black-box tests for the exact event/state/counter order.
- Add assertions that the next post is not requested before settlement acknowledgement.
- Capture the current failure as a regression: a run must not succeed when source rows say ingested
  but canonical IDs are null.

### 2. Create The Minimal Schema-1 Runtime

- Create the revised schema-1 database fresh; there is no conversion.
- Add `source_post_attempt`, `source_file_attempt`, and `gallery_job` per the Concrete Schema
  Requirements section.
- Collapse or delete state columns/tables made redundant by the new attempt model.
- Add constraints/indexes enforcing one current post/query and valid terminal outcomes where SQLite
  can enforce them.
- Reject every incompatible database without mutation. Do not update the temporary converter; it
  is deleted during cleanup.

### 3. Implement The Rust Post Processor

- Implement the state machine as one module with explicit transition functions.
- Make illegal transitions errors, not no-ops.
- Acquire and release the subscription execution lease transactionally for full and manual-query
  runs; full runs claim their child queries serially.
- Own counters as queries over attempt outcomes.
- Invoke canonical ingest directly after complete download staging.
- Verify root, media, provenance, collection vector, and thumbnail before `added` commits.
- Add restart recovery for every non-terminal state.

### 4. Replace The Python Protocol

- One reusable gallery-dl service process driven by the Rust worker (or a strictly bounded pool if
  gallery-dl isolation proves necessary — never a process per query), all requests through the one
  shared per-domain limiter.
- Import vendored gallery-dl APIs directly.
- Implement `NEXT_POST`, `DOWNLOAD_CURRENT_POST`, and `ACK_POST` pull semantics.
- Remove CLI-style whole-query execution and archive-based settlement.
- Add protocol tests proving the Python iterator does not advance before acknowledgement.

### 5. Integrate Exact Duplicates And Tombstones

- Preserve the existing standalone exact-hash tag fanout behavior.
- Return an explicit canonical ingest outcome: `Created(root_ids)` (one or more, matching the
  multi-root attempt model) and/or `ExistingMetadataUpdated(root_ids)` — a single post may create
  roots while also updating exact-duplicate roots, so the outcome carries both sets rather than
  inferring creation from IDs.
- Map a non-empty `Created` set to post added.
- Make explicit gallery reimport atomically override relevant tombstones.
- Add collection-specific physical dedup tests.

### 6. Replace Gallery Import Ownership

- Add gallery job IPCs for create, inspect/progress, retry, cancel, and dismiss.
- Route it through the shared post processor.
- Remove transient subscription creation/deletion for galleries.
- Remove renderer-side success inference and cleanup.

### 7. Connect Durable Progress

- Publish one compact source-progress invalidation after each committed transition.
- Query progress from attempt rows.
- Keep stable gallery totals once discovered.
- Update subscription cards, details, history, notifications, and gallery rows to consume only this
  DTO.

### 8. Cut Providers Over One By One

Provider inventory being cut over: baraag, danbooru, deviantart, e621, ehentai (incl. exhentai
URLs), fanbox, furaffinity, gelbooru, hentaifoundry, idolcomplex, konachan, newgrounds, onlyfans
(OF-Scraper bridge, same engine contract), patreon, pixiv, pixivuser, rule34, safebooru, sankaku,
subscribestar, tumblr, twitter, yandere.

Webtoons is explicitly excluded: delete its backend adapter, metadata logic, auth catalog entry,
bridge handling, and tests rather than only hiding it in TypeScript.

For every provider — gallery-dl and OF-Scraper alike (OnlyFans runs the same contract suite,
plus extractor-specific protocol tests for its purchased/messages/feed partitions):

- Run the same fake/contract suite through its adapter.
- Verify auth material reaches gallery-dl without changing auth ownership.
- Verify strict post boundaries and request pacing in traces.
- Verify canonical tags: `creator`, `character`, `series`, `species`, and `rating` are specialized;
  supported Hydrus namespaces remain canonical; all other source tags are general.
- Verify descriptions strip generic HTML/BBCode markup before notes are stored.
- Certify reset and rerun with exact duplicates.

Do not add a provider-specific orchestration path to make certification pass.

### 9. Delete Replaced Paths

Delete in the same cutover series:

- Gallery-dl archive authority and archive reset logic.
- Whole-query CLI-style bridge execution.
- Subscription-to-ingest-job state mirroring.
- Transient gallery subscriptions and renderer cleanup.
- Stale state reconciliation that exists only to repair old parallel authorities.
- Compatibility DTO mappers and alternate progress counters.
- The temporary schema converter (`picto-schema-v1-convert`) — incompatible libraries are rejected
  without mutation, not converted.
- The webtoons provider: backend adapter, metadata logic, auth catalog entry, bridge handling, and
  tests.

There must be one production subscription path after cutover and no dual-write period.

### 10. Verification And Tracing

- Run focused Rust, Python protocol, and renderer contract tests once per coherent slice, not after
  every edit.
- Run a final release-mode suite and `git diff --check` before handoff.
- Trace one public provider and one authenticated provider end to end.
- Compare request timestamps against the provider-specific per-domain pacing policy.
- Flag any idle gap over 250 ms that is not network pacing, file transfer, thumbnail generation, or
  canonical ingest.
- Perform UI verification only with the user's explicit coordination; do not compete for control of
  the running application.

## Required Contract Tests

1. A two-file post emits traversed, file 1, file 2, added, then requests the next post.
2. A no-media post emits traversed then skipped and does not consume the added-post limit.
3. An exact duplicate emits traversed/download progress as applicable, transfers standalone tags,
   emits skipped, and continues.
4. A collection with duplicate physical files still creates one coherent collection when
   structurally new.
5. A failed media item creates a warning/problem with the post link and does not deadlock.
6. Reset clears only the selected subscription's attempts, counts, cursor, problems, and staging.
7. Two subscriptions with identical queries never share progress/history or poison each other's
   traversal.
8. Crash after traversed resumes the same post.
9. Crash after one of several files resumes the same post without advancing.
10. Crash after canonical commit but before acknowledgement detects the canonical root and settles
    exactly once.
11. `ingested` with null media/root IDs is rejected and cannot produce run success.
12. Gallery progress advances per file, retains its total, publishes one Inbox root only after the
    cover thumbnail exists, and cannot report success without that root.
13. Automatic subscriptions respect tombstones; explicit gallery import can atomically reimport.
14. Post limit 2 adds exactly two posts while allowing any number of no-media/duplicate skips.
15. No provider request for post N+1 begins before post N reaches a terminal committed outcome.
16. A post added with warnings counts as added: `posts_added` increments and the post limit does
    not overrun.
17. A run where every discovered post is already known or has no usable media terminates (at
    exhaustion, settled frontier, or the reported safety bound) instead of traversing forever.
18. The cursor commits before ACK and never advances on a failed outcome.
19. Multiple active queries on one domain obey one shared per-domain limiter.
20. Gallery progress uses `GalleryJob` ownership without fake query IDs.
21. Reset never changes another subscription with the identical source query.
22. Crash recovery after canonical commit settles exactly once without creating another root.
23. A group-scoped provider cursor (e.g. purchased/messages/feed) never clamps discovery in a
    later group: content newer than an earlier group's cursor timestamp is still discovered and
    settled.
24. Starting a manual query while its subscription is running, or starting the full subscription
    while one of its queries is running, is rejected without creating a queued or duplicate run.
25. A full-subscription run executes its eligible queries serially, and Stop durably releases the
    subscription lease only after the active worker is cancelled.

## Acceptance Criteria

- Every provider uses the same Rust post processor.
- Every gallery-dl provider uses the same pull/ack Python protocol.
- Authentication code is unchanged by this project.
- One source post is in flight per query.
- One execution is active per subscription; full runs execute their queries serially.
- Later-post prefetch is impossible by construction.
- Added/skipped/downloaded counters match durable canonical outcomes.
- No success can exist without verified canonical output.
- Gallery image progress updates throughout the download.
- Same-query subscriptions are fully isolated.
- Reset is deterministic and owner-scoped.
- The provider-specific per-domain limiter is the only deliberate request delay.
- Old archives, transient gallery subscriptions, duplicate progress systems, and ingest-state
  mirroring are deleted.
- Focused crash, duplicate, reset, gallery, provider, and UI contract tests pass.

## Non-Goals

- Reworking authentication.
- Reworking the greenfield root/bitmap database model.
- Changing cloud, FTS, AI tagging, duplicate-review algorithms, or thumbnail generation.
- Adding a schema migration chain.
- Preserving old subscription internals for compatibility.
- Adding speculative provider capabilities not exposed by the UI.
