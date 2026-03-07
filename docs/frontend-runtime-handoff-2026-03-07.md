# Frontend Runtime Handoff (2026-03-07)

## Purpose

This document is for the frontend implementer working against the backend
runtime/state re-architecture.

The goal is to stop encoding product semantics and refresh behavior in the
renderer. The frontend should consume backend truth, not reconstruct it.

## Non-Negotiable Rules

1. The renderer does not define scope semantics.
2. The renderer does not decide which model facts exist after a mutation.
3. The renderer does not emit panel-level refresh intent like "refresh grid"
   or "refresh inspector" as a primary contract.
4. Background task progress and mutation receipts are separate event families.
5. A visible view is a derived resource, not truth.

## What The Frontend Must Treat As Truth

### 1. Model facts

The backend is authoritative for:

1. entity ids that changed
2. folder membership changes
3. tag changes
4. status changes
5. smart-folder definition changes
6. task lifecycle and progress state

If the frontend is deriving any of those from UI actions or by guessing from a
button click, it is doing the wrong job.

### 2. Scope semantics

The backend is authoritative for the meaning of:

1. `system:all`
2. `system:inbox`
3. `system:untagged`
4. `system:uncategorized`
5. tag-search semantics
6. folder union/exclusion semantics
7. `select all`

The frontend must never reimplement these rules locally.

## What The Frontend Should Own

The frontend owns:

1. active view and navigation state
2. which derived resources are currently mounted or cached
3. how stale resources are refetched
4. presentation of task progress
5. view transitions and pending/loading display

The frontend does **not** own the business meaning of the underlying data.

## Correct Mental Model

Use this model:

1. backend mutates truth
2. backend emits mutation facts
3. frontend marks derived resources stale
4. frontend refetches those resources
5. components render the refreshed resources

That means:

1. entities are truth
2. folders/tags/status relationships are truth
3. grids are derived resources
4. metadata panels are derived resources
5. sidebar snapshots/counts are derived resources
6. selection summaries are derived resources

Do not think in terms of "which UI panels should I refresh?" Think in terms of
"which derived resources depend on the changed facts?"

## Required Frontend Resource Model

The frontend should standardize on resource keys such as:

1. `grid:<scope_key>`
2. `metadata:<entity_id>`
3. `sidebar:snapshot`
4. `sidebar:counts`
5. `selection:<selection_key>`
6. `tasks:active`

These resource keys must be the unit of staleness and refetch, not individual
components.

## What To Stop Doing

1. Do not special-case refreshes per component after each action.
2. Do not let controllers manually invalidate inspector + grid + sidebar + tag
   UI separately.
3. Do not re-run local scope logic for select-all or selection summary.
4. Do not keep feature-specific event bridges that interpret the same backend
   mutation differently.
5. Do not treat "grid invalidation" as the primitive. The primitive is model
   facts; grid staleness is derived from them.

## Required Frontend Integration Points

### 1. Runtime store

There should be one runtime synchronization store responsible for:

1. receiving task events
2. receiving mutation receipts
3. maintaining sequence tracking
4. refreshing snapshots on recovery or gap detection
5. computing stale resource keys from model facts

Feature stores can consume that runtime store, but they should not build
parallel global event logic.

### 2. Snapshot recovery

The frontend must assume it can lose listeners because of:

1. hot reload
2. window reopen
3. secondary window mount timing
4. event bridge teardown/rebind

So it must recover from:

1. `get_runtime_snapshot`
2. any domain-specific snapshot query needed for a stale resource

No feature should depend on "I happened to receive every event in real time."

### 3. Task UI

Subscriptions, PTR, flows, and similar long-running work should be rendered
from task snapshots and task events only.

Do not infer task status from:

1. button state
2. optimistic local counters
3. sidebar count changes
4. indirect mutation events

## Specific Guidance For Current Problem Areas

### Select all

The selection layer should consume the current resolved scope from the backend.
It must not have a second interpretation of visibility.

### Auto-tag folders

Moving an item into an auto-tagged folder should not require frontend feature
code to know:

1. inspector tags changed
2. folder membership changed
3. sidebar counts changed
4. smart folders may have changed

That should fall out of:

1. one mutation receipt from the backend
2. one resource dependency map
3. one runtime store marking resources stale

### Grid updates

It is fine for a grid to stay visible briefly while stale, but the staleness
decision must be resource-based, not hard-coded per mutation handler.

### Sidebar updates

Sidebar counts and tree snapshots should be refreshed because model facts say
they are stale, not because a specific button handler remembered to emit a
sidebar event.

## Frontend Acceptance Criteria For This Re-Architecture

1. No feature controller manually invalidates multiple UI panels after a
   mutation.
2. `select all` uses backend-resolved scope semantics, not duplicated local
   rules.
3. Task progress remains correct after reload or listener loss.
4. Stale derived resources are refreshed through one runtime/resource path.
5. Components consume refreshed resources; they do not invent invalidation
   policy.

## Practical Planning Advice For Frontend Work

If you are implementing against these backend PBIs, plan the renderer work in
this order:

1. consume typed mutation receipts and task events in one runtime store
2. define resource keys and one dependency-based staleness reducer
3. migrate grid/inspector/sidebar/selection stores to resource consumption
4. delete per-feature event bridges and manual invalidation hooks
5. only then simplify feature controllers/components

Do not start by editing individual panels. Start by fixing the runtime/store
ownership model.
