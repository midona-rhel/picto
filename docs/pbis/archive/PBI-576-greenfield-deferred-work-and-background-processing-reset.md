# PBI-576: Greenfield deferred work and background processing reset

## Priority
P1

## AI-generated caveat
This document supersedes the old narrow deferred-work queue idea by treating background work as a shared platform concern. The implementing engineer should keep it small, explicit, and durable.

## Lifecycle
- `Implemented` when one background-work platform exists with durable queue/task semantics.
- `Activatable` when `PBI-567` and `PBI-568` are implemented enough that domains can schedule work through the new boundaries.
- `Activated` when the intended heavy-work flows use the shared background-work platform by default.
- `Legacy removed` when replaced domain-specific queue/runtime paths for that activated slice are deleted.

Activation depends on:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)

## Problem
The application still treats several heavy or long-running operations as special cases instead of one background-processing model.

Current problems:
- thumbnails, dominant colors, phash, and similar work still feel like adjacent mechanisms
- some job-like flows have central task state while others still carry their own runtime semantics
- retry, lease, progress, and restart behavior are not uniformly defined
- heavy work still leaks into unrelated domains instead of being routed through one background-work layer

## Product model to encode
Background work should reflect these truths:
- heavy non-interactive work is queued and processed asynchronously
- jobs are durable across restart
- work items are typed and idempotent
- progress, retry, and failure state are part of the platform
- normal user-facing domains schedule work; they do not implement their own queue semantics

## Locked decisions

### 1. One background-work platform
Use one shared deferred/background processing subsystem for:
- thumbnail generation
- preview frame generation
- dominant color extraction
- perceptual hash computation
- AI tagging
- any future heavy analysis work

### 2. Typed work items
Every work item must carry:
- work kind
- target identity
- payload or parameters
- status
- retry count
- next-attempt time
- last error
- timestamps

Do not use one vague queue row with untyped semantics.

### 3. Durable lease/retry semantics
Workers must use explicit lease/retry behavior.

That includes:
- claiming work
- lease expiry / crash recovery
- bounded retries
- backoff
- poison / permanent failure classification

### 4. Domains schedule work, they do not own it
Import, subscriptions, duplicates, media delivery maintenance, and AI features may request background work.

They must not each reinvent:
- queue persistence
- retry semantics
- worker heartbeats
- job progress plumbing

## Required subsystem shape

### Main APIs
The background-work subsystem should expose a small interface such as:
- `enqueue_deferred_work(...)`
- `get_deferred_work_summary()`
- `get_deferred_work_items(filter)`
- `retry_deferred_work(target)`
- optional `cancel_deferred_work(target)`

### Worker model
Workers should be typed executors over the queue:
- derivative worker
- analysis worker
- AI tagging worker
- any other clearly separate executor type

The queue model stays shared even if executors differ.

## Relationship to other reset PBIs
- PBI-573 ingest schedules work here
- PBI-575 subscriptions use this for heavy follow-up processing, not for gallery-dl network runs themselves
- PBI-569 must not hide derivative generation inside media reads
- PBI-578 bulk actions may enqueue follow-up work for many entities at once
- this PBI supersedes the narrow intent of [PBI-545-deferred-work-queue.md](./docs/pbis/active-alpha/PBI-545-deferred-work-queue.md)

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- background work uses one durable shared subsystem
- work kinds are typed and explicit
- retry and lease semantics survive restart
- normal domains schedule background work instead of implementing their own queue behavior
- progress and failure state are queryable through one small API

## Tests
Required tests:
- enqueue and claim work item
- crash/restart lease recovery
- bounded retry with backoff
- poison/permanent failure classification
- ingest-scheduled derivative work
- delete all worker state except authoritative stored rows and rebuild behavior where applicable
