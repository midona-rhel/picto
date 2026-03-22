# PBI-553: Consolidate Tags Controller And Tag-Driven Consequences

## AI-Generated Caveat
This PBI is AI-generated and assumes the current tags slice still mixes direct backend calls, UI-owned undo, and coarse post-change refresh. The engineer should confirm the remaining mixed ownership before coding.

## Priority
P0

## Problem
Tag actions are still scattered across tag manager, grid actions, inspector actions, and selection helpers. Tag-driven side effects are also architecturally important because they can affect smart folders and scope membership.

## Goal
Make `tagsController` the single public entrypoint for tag reads and writes, and make the tag slice rely on backend-declared consequences instead of broad refresh behavior.

## Atomicity Rule
This PBI should finish the tags slice without dragging in general files/folders cleanup except where a tag action directly touches them.

## Scope

### Controller
- [src/controllers/tagsController.ts](./src/controllers/tagsController.ts)

### Known consumers
- [src/features/tags/components/TagManager.tsx](./src/features/tags/components/TagManager.tsx)
- [src/features/tags/components/TagSelectPanel.tsx](./src/features/tags/components/TagSelectPanel.tsx)
- [src/features/tags/components/TagRelationsModal.tsx](./src/features/tags/components/TagRelationsModal.tsx)
- [src/features/grid/hooks/useGridItemActions.ts](./src/features/grid/hooks/useGridItemActions.ts)
- [src/features/inspector/hooks/useInspectorChangeActions.ts](./src/features/inspector/hooks/useInspectorChangeActions.ts)

## Required Reads
- search
- get all
- get file tags
- paginated tags
- namespace summary
- relations
- files-by-tags

## Required Writes
- add/remove tags for hashes
- add/remove tags for selection
- rename tag
- delete tag
- merge tags
- manage alias
- manage implication

## Required Eager UI Behavior
- tag chips disappear/appear immediately in the touched UI
- local visible tag lists update immediately

Derived consequences such as smart-folder membership should reconcile from backend `runtime/state_changed`.

## Look For Adjacent Improvements
- remove duplicated tag parsing/normalization paths
- simplify batch vs single-item tag action APIs
- tighten namespace-summary usage if the UI only needs smaller derived data
- collapse near-duplicate tag operations that differ only by small naming or wrapper variations
- remove redundant domain repetition in controller method names, because the controller already provides the tag context

## Non-Goals
- general files metadata writes
- folder/collection membership

## Acceptance Criteria
1. All tag reads/writes route through `tagsController`.
2. No feature-local backend access remains for this slice.
3. Tag chips update eagerly in direct UI surfaces.
4. Undo/redo is controller-owned if PBI-559 is complete.

## Validation
- add/remove tags from grid
- add/remove tags from inspector
- rename/delete/merge/alias/implication in tag manager
- confirm smart-folder and `untagged` consequences arrive from backend state change
