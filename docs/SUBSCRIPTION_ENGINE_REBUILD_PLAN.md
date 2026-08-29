# Picto Subscription Engine Rebuild

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
- One-at-a-time source-post traversal.
- Per-post download staging and progress.
- Canonical post ingestion.
- Added, skipped, warning, and failed outcomes.
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
discovered/downloading -> skipped
discovered/downloading/ingesting -> warning
discovered/downloading/ingesting -> failed
```

Rules:

- `traversed` increments when `discovered` commits.
- `files downloaded` increments after each staged file is durable and verified to exist.
- `added` commits only after canonical ingest returns and the engine verifies a live root and every
  expected source-to-media association.
- `skipped` means no usable media, an already-settled exact duplicate, or another explicit
  non-failure terminal reason.
- A warning is a completed post with one or more non-fatal media failures. Problems retain the post
  link and concise reason.
- A failed post/run is reserved for a condition that prevented a valid terminal outcome.
- The next post cannot be requested before the current post reaches a terminal state.
- The configured post limit counts `added` only. Skips never consume it.

### Minimal Durable Tables

Keep stable definitions and history, but eliminate parallel state ownership.

Retain or replace with direct schema-1 equivalents:

- `subscription`: definition and schedule.
- `subscription_query`: provider query, cursor, grouping choice, pause state.
- `source_run`: one manual or scheduled execution.
- `source_run_query`: one query execution and aggregate terminal status.
- `source_post`: stable provider post identity and normalized metadata.
- `source_item`: stable provider media identity and canonical media reference.
- `source_post_attempt`: the sole persisted post state for a run/query.
- `source_file_attempt`: staged path, bytes, and download outcome for current-post progress.
- `subscription_issue`: user-visible warnings/problems.
- `gallery_job`: transient gallery-import owner, separate from subscription definitions.

Do not use a gallery-dl archive database as a second history authority. Picto's source identities,
provenance, and post outcomes already provide the required idempotency.

Do not route subscription posts through a second durable `ingest_job` lifecycle. Once a post is
fully downloaded, the source worker invokes the canonical library ingest API directly and stores
the result in the same post-attempt transition. The generic ingest queue may remain for unrelated
manual/background imports if those operations still need it.

### State Invariants

Enforce these in code and tests:

- `added` requires `source_post.root_item_id` to reference a live `library_root`.
- Every successfully downloaded usable source item in an added post has a live
  `source_item.media_item_id`.
- `source_item.state = ingested` with a null media ID is invalid and must never be persisted.
- A post cannot be both added and skipped.
- A run cannot succeed while any started post is non-terminal.
- A query cannot have more than one non-terminal post attempt.
- A later post cannot have a download attempt while an earlier post is non-terminal.
- Progress counts derive only from attempt rows, never from inferred combinations of unrelated
  tables.
- Renderer cleanup cannot alter or manufacture backend success.

## gallery-dl Integration

### Use The Library, Not CLI Orchestration

Run one Python worker process per active query and import the vendored `gallery_dl` package
directly. The process is an extractor service, not a durable worker and not an authority.

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
Rust -> ACK_POST(added | skipped | warning)
Rust -> NEXT_POST(next cursor)
```

The Python iterator must not be advanced after the current post boundary until `ACK_POST` arrives.
If a gallery-dl extractor internally fetches the next post before yielding the current boundary,
adapt that provider inside its gallery-dl site adapter by using a one-post cursor/page window. Do
not weaken the global engine or add local-provider shims.

### Request Pacing

- Apply the one-request-per-second/domain policy at the actual HTTP request boundary.
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

- `gallery_job` owns URL, service, run state, expected image total, downloaded count, canonical
  root ID, warning/error, and timestamps.
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

## Progress And UI Contract

Expose one backend progress DTO derived from durable attempt state:

```text
SourceRunProgress {
  run_id,
  query_id,
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

- Edit schema generation 1 directly; do not add a migration.
- Add `source_post_attempt`, `source_file_attempt`, and `gallery_job` as needed.
- Collapse or delete state columns/tables made redundant by the new attempt model.
- Add constraints/indexes enforcing one current post/query and valid terminal outcomes where SQLite
  can enforce them.
- Incompatible libraries must fail without mutation; update the one-shot converter separately.

### 3. Implement The Rust Post Processor

- Implement the state machine as one module with explicit transition functions.
- Make illegal transitions errors, not no-ops.
- Own counters as queries over attempt outcomes.
- Invoke canonical ingest directly after complete download staging.
- Verify root, media, provenance, collection vector, and thumbnail before `added` commits.
- Add restart recovery for every non-terminal state.

### 4. Replace The Python Protocol

- Keep one Python process per running query.
- Import vendored gallery-dl APIs directly.
- Implement `NEXT_POST`, `DOWNLOAD_CURRENT_POST`, and `ACK_POST` pull semantics.
- Remove CLI-style whole-query execution and archive-based settlement.
- Add protocol tests proving the Python iterator does not advance before acknowledgement.

### 5. Integrate Exact Duplicates And Tombstones

- Preserve the existing standalone exact-hash tag fanout behavior.
- Return an explicit canonical ingest outcome: `Created(root_id)` or
  `ExistingMetadataUpdated(root_ids)` rather than inferring creation from IDs.
- Map only `Created` to post added.
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

For each gallery-dl provider:

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

There must be one production subscription path after cutover and no dual-write period.

### 10. Verification And Tracing

- Run focused Rust, Python protocol, and renderer contract tests once per coherent slice, not after
  every edit.
- Run a final release-mode suite and `git diff --check` before handoff.
- Trace one public provider and one authenticated provider end to end.
- Compare request timestamps against the one-request-per-second/domain policy.
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

## Acceptance Criteria

- Every provider uses the same Rust post processor.
- Every gallery-dl provider uses the same pull/ack Python protocol.
- Authentication code is unchanged by this project.
- One source post is in flight per query.
- Later-post prefetch is impossible by construction.
- Added/skipped/downloaded counters match durable canonical outcomes.
- No success can exist without verified canonical output.
- Gallery image progress updates throughout the download.
- Same-query subscriptions are fully isolated.
- Reset is deterministic and owner-scoped.
- The one-request-per-second/domain limiter is the only deliberate request delay.
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
