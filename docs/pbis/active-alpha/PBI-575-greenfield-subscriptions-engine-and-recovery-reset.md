# PBI-575: Greenfield subscriptions engine and recovery reset

## Priority
P1

## Current audit status (2026-08-03)

Partially implemented and release-blocking. Durable runs, query jobs, download attempts,
ingest handoff, and a rebuilt frontend exist. The remaining work is concrete:

- issue identity includes mutable message text and permits duplicates for null query ids
- persisted issues do not yet carry the full severity/recoverability/log contract below
- transport still constructs `SubscriptionRuntimeService` directly instead of calling one
  bounded subscription service through the engine
- the site-specific PBIs were archived; supported sites are now one verification matrix owned
  by this PBI

Completed release slice:
- retry validation and queued retry execution use one indexed lookup over all unresolved matching
  attempts; no newest-500 scan remains

## AI-generated caveat
This document is based on an in-repo audit of the current subscriptions CRUD, run orchestration, gallery-dl runner, and runtime progress/state handling. It is intentionally decisive. The main goal is to stop treating too many failures as opaque and terminal.

## Lifecycle
- `Implemented` when `SubscriptionService` exists with definition/run/issue boundaries.
- `Activatable` when `PBI-568`, `PBI-573`, and `PBI-576` are implemented enough for subscriptions to use the new engine, ingest, and background-work layers.
- `Activated` when the live subscription flow uses the new subscription service by default.
- `Legacy removed` when replaced subscription transport/runtime paths for that activated slice are deleted.

The engine, ingest, and background-work foundations are live. Their historical plans are in
`docs/pbis/archive/`; they are no longer scheduling dependencies.

## Problem
The current subscription system works, but too much of its runtime behavior is still modeled as “run something, maybe fail, maybe stop.”

Current problems:
- subscriptions still expose a large CRUD and control surface instead of one bounded subsystem
- gallery-dl failures are classified too coarsely and mainly survive as short-lived status strings
- retryable and non-retryable outcomes are not modeled strongly enough
- broken queries, broken credentials, extractor changes, rate limits, and transient network problems are not persisted as first-class issues
- subscription import still feels adjacent to ingest instead of explicitly delegating to the ingest pipeline

## Current implementation status
- Subscription downloads now enqueue durable ingest work instead of importing inline in the sync loop.
- Exact file-hash duplicates in subscription reruns now settle through the ingest queue as successful reuse instead of failed import rows.
- Queue-backed ingest progress now distinguishes queued, ingesting, imported, reused, and failed work at the runtime progress layer.
- Startup repair resets previously poisoned exact-hash duplicate rows for retry so they can settle cleanly and release temp files.
- Remaining parity work is mostly around richer issue classification and final runtime reporting, not the shared ingest handoff itself.

## Product model to encode
The subscription subsystem should reflect these truths:
- subscription definitions are persistent domain records
- subscription runs are task executions over those definitions
- query and site failures should produce persisted issues, not just log lines
- most failures are recoverable, retryable, or reviewable, not globally “unrecoverable”
- gallery-dl is an implementation detail behind one subscription runtime
- imported media flows through the normal ingest pipeline
- subscriptions are a bounded backend service, not just a folder under the library database

## Locked decisions

### 1. Split definition state from runtime state
Model subscriptions as:
- definition records
- run attempts
- persisted issues
- progress snapshots
- credential health

Do not collapse all of that into the subscription row itself.

Locked rule:
- this subsystem should be implemented behind a `SubscriptionService`
- `SubscriptionService` may keep its own storage for definitions, runs, issues, logs, and credential health
- it should not be forced into `LibraryDatabase` if that makes ownership blurrier

### 2. Persist issues from gallery-dl and runtime failures
When a run encounters a meaningful problem, persist a structured issue record.

The issue model should capture:
- subscription or query identity
- site category
- issue kind
- severity
- recoverability
- first seen / last seen timestamps
- retry policy or next suggested retry time
- short machine-readable classification
- operator-facing message
- retained log excerpt or structured detail payload

### 3. Recoverable failures stay recoverable
The system must distinguish at least:
- credential/auth blocked
- rate limited / backoff required
- transient network failure
- extractor/schema drift
- metadata validation failure
- import-side failure
- user cancelled
- permanently invalid definition

Do not flatten all of these into one generic failure status.

### 4. Gallery-dl logs are classification input
The runtime must use gallery-dl stderr/stdout and run context to classify failures.

That means:
- keep structured run logs or retained excerpts
- classify failures into persisted issue kinds
- allow later review and retry without losing the reason

### 5. Subscription import delegates to ingest
Downloaded items from subscriptions must go through the main ingest pipeline.

Subscriptions are not allowed to keep their own special import semantics once the ingest reset lands.

### 6. Subscription control surface must shrink
The public subsystem should collapse around:
- definition CRUD
- query CRUD
- run control
- issue review
- progress/runtime reads
- credential management

Do not keep multiplying narrow transport commands around the same conceptual operations.

## Required subsystem shape

### Main engine categories
The subsystem should expose clear categories such as:
- `subscriptions.query_definitions(...)`
- `subscriptions.mutate_definition(...)`
- `subscriptions.run_control(...)`
- `subscriptions.get_runtime_state(...)`
- `subscriptions.list_issues(...)`
- `subscriptions.resolve_issue(...)`
- `subscriptions.credentials(...)`

### Persisted runtime records
The new runtime model should include explicit records such as:
- `subscription_run`
- `subscription_query_run`
- `subscription_issue`
- `subscription_issue_event`
- credential-health rows

Exact table names can be improved, but the separation is not optional.

The engine should talk to this subsystem through a typed service boundary. It should not know or care whether the subscription service stores its state in the library database, a separate SQLite database, or another local store.

## Boundaries
- Subscription downloads use the live ingest queue.
- Scheduling and retries use the live background-work layer.
- Dispatch should call one bounded subscription service through the engine.
- Media delivery remains outside this subsystem.

## Acceptance criteria
This PBI is complete only when:
- subscriptions are modeled as one bounded subsystem instead of scattered handlers
- gallery-dl failures produce persisted structured issues
- recoverable vs blocked vs terminal outcomes are explicit
- runtime logs are retained enough to explain failures later
- subscription downloads go through the shared ingest pipeline
- the public subscription control surface is materially smaller and clearer than the current transport surface

## Tests
Required tests:
- auth failure becomes persisted blocked issue
- rate limit becomes persisted retryable issue with backoff semantics
- transient network error becomes retryable issue
- extractor drift / metadata mismatch becomes reviewable issue
- successful run clears or resolves matching transient issues
- subscription import uses the ingest pipeline
- run progress and issue review survive process restart
