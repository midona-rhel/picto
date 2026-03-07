# PBI-402: Frontend runtime sync store and derived resource model

## Priority
P0

## Audit Status (2026-03-07)
Status: **Not Implemented**

Backend dependencies:

1. `PBI-300` canonical scope semantics engine
2. `PBI-302` fact-based mutation receipts and model-level invalidation
3. `PBI-303` derived resource dependency map from model facts
4. runtime communication implementation work under `PBI-234`

## Problem
The renderer still reconstructs too much truth from UI behavior and feature-local
event handling.

Current problems:

1. feature code still reasons in terms of panel refreshes instead of model facts
2. event ownership is split across `eventBridge`, feature stores, and view hooks
3. task progress, mutation receipts, and cache invalidation are not yet unified
4. the frontend can still lose correctness after reload or listener churn unless
   it happened to receive all events live

This makes frontend/backend alignment fragile even if the backend runtime model
improves.

## Goal
Make the frontend consume backend truth rather than reconstruct it.

The renderer should:

1. receive task events and mutation receipts in one runtime synchronization store
2. derive stale resource keys from model facts
3. refetch derived resources deterministically
4. stop encoding product semantics in feature-local refresh code

## Scope
- `src/stores/eventBridge.ts`
- `src/stores/taskRuntimeStore.ts`
- current runtime/resource invalidation code under `src/runtime/`
- grid, inspector, sidebar, selection, and task consumers

## Required Frontend Model

The frontend should standardize on derived resource keys such as:

1. `grid:<scope_key>`
2. `metadata:<entity_id>`
3. `sidebar:snapshot`
4. `sidebar:counts`
5. `selection:<selection_key>`
6. `tasks:active`

These keys are the unit of staleness and refetch, not individual components.

## Non-Negotiable Rules

1. The renderer does not define scope semantics.
2. The renderer does not decide which model facts exist after a mutation.
3. The renderer does not emit panel-level refresh intent as the primary contract.
4. Background task progress and mutation receipts are separate event families.
5. A visible view is a derived resource, not truth.

## Implementation
1. Introduce one runtime synchronization store responsible for:
   - receiving task events
   - receiving mutation receipts
   - maintaining sequence tracking
   - refreshing snapshots on recovery or gap detection
   - computing stale resource keys from model facts
2. Stop treating grid/sidebar/inspector refresh as primary concepts in event handling.
3. Migrate feature stores to consume stale resources and refreshed snapshots rather than bespoke invalidation behavior.
4. Support snapshot recovery so the renderer can recover after:
   - hot reload
   - window reopen
   - listener teardown/rebind
5. Render subscriptions/PTR/flows from task snapshot + task event state, not from indirect UI clues.

## What To Stop Doing
1. Do not special-case refreshes per component after each action.
2. Do not let controllers manually invalidate inspector + grid + sidebar + tag UI separately.
3. Do not re-run local scope logic for select-all or selection summary.
4. Do not keep feature-specific event bridges that interpret the same backend mutation differently.
5. Do not treat “grid invalidation” as the primitive. The primitive is model facts.

## Acceptance Criteria
1. One runtime store owns mutation receipt and task synchronization.
2. `select all` uses backend-resolved scope semantics, not duplicated local rules.
3. Task progress remains correct after reload or listener loss.
4. Derived resources are marked stale from model facts and refetched through one resource path.
5. Components consume refreshed resources; they do not invent invalidation policy.

## Test Cases
1. Reload the renderer during active subscriptions and recover task state from snapshot + events.
2. Move an item into an auto-tagged folder and confirm grid/sidebar/metadata staleness comes from model facts rather than a feature-specific refresh sequence.
3. `select all` matches backend-resolved visible scope exactly.
4. Sidebar counts and tree refresh via resource invalidation, not handler-specific refresh code.

## Risk
High. This is the main frontend counterpart to the backend runtime/state rearchitecture and touches the app’s synchronization model.
