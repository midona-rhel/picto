# PBI-555: Consolidate Collections Controller

## AI-Generated Caveat
This PBI is AI-generated from a mixed frontend/backend access audit. The engineer should check whether any collection-like flows still hide under folder terminology before closing this PBI.

## Priority
P1

## Problem
Collection reads and writes still risk being split across collection screens, inspector actions, and context actions. Membership changes also overlap heavily with direct visible grid consequences.

## Goal
Make `collectionsController` the single public entrypoint for collection reads and writes.

## Atomicity Rule
This PBI should finish collections only. Do not absorb folder or smart-folder cleanup except where a shared helper must be touched.

## Scope

### Controller
- [src/controllers/collectionsController.ts](./src/controllers/collectionsController.ts)

### Known consumers
- [src/features/collections/components/Collections.tsx](./src/features/collections/components/Collections.tsx)
- [src/features/inspector/hooks/useInspectorState.ts](./src/features/inspector/hooks/useInspectorState.ts)
- [src/shared/components/context-actions/imageActions.tsx](./src/shared/components/context-actions/imageActions.tsx)

## Required Reads
- summary
- list
- member hashes

## Required Writes
- create
- update
- delete
- add members
- remove members
- reorder members
- add tags
- remove tags

## Required Eager UI Behavior
- add/remove collection membership updates current visible collection scope immediately where obvious
- collection name changes update visible labels immediately

## Look For Adjacent Improvements
- collapse collection and collection-entity naming where misleading
- remove duplicated reorder payload shaping
- simplify collection tags flow if it duplicates generic tag UI
- collapse near-duplicate collection APIs that are effectively the same operation under different names
- remove redundant domain repetition in method names inside `collectionsController`

## Acceptance Criteria
1. All collection reads/writes route through `collectionsController`.
2. No raw backend access remains in collection UI.
3. Direct visible collection effects update eagerly.
4. Undo/redo is controller-owned if PBI-559 is complete.

## Validation
- create/update/delete collection
- add/remove/reorder collection members
- add/remove collection tags
