# PBI-552: Consolidate Files Controller And Direct Entity UI Updates

## AI-Generated Caveat
This PBI is AI-generated from a code audit and recent partial migration work. The current implementation may already cover parts of this slice. The engineer should finish the slice fully rather than preserving partial mixed ownership.

## Priority
P0

## Problem
File and entity actions still have mixed ownership across grid hooks, inspector hooks, context actions, and controller helpers. Even where a files controller exists, it may still be too thin, and visible UI updates are not consistently owned there.

## Goal
Make `filesController` the single public entrypoint for file/entity reads and writes, with eager direct UI updates for obvious visible consequences.

## Atomicity Rule
This PBI should complete the files/entity slice without bundling tags, folders, or collections beyond the minimum required for direct consequences.

## Scope

### Controller
- [src/controllers/filesController.ts](./src/controllers/filesController.ts)

### Known consumers
- [src/features/grid/hooks/useGridStateActions.ts](./src/features/grid/hooks/useGridStateActions.ts)
- [src/features/grid/hooks/useGridData.ts](./src/features/grid/hooks/useGridData.ts)
- [src/features/inspector/hooks/useInspectorChangeActions.ts](./src/features/inspector/hooks/useInspectorChangeActions.ts)
- [src/features/inspector/hooks/useInspectorState.ts](./src/features/inspector/hooks/useInspectorState.ts)
- [src/features/viewer/components/MediaView.tsx](./src/features/viewer/components/MediaView.tsx)
- [src/shared/components/context-actions/imageActions.tsx](./src/shared/components/context-actions/imageActions.tsx)

## Required Reads
- entity details
- metadata
- metadata batch
- grid pages
- selection summary
- similar-files lookup
- path/thumbnail-path resolution

## Required Writes
- status changes
- selection status changes
- delete / delete selection
- name
- rating
- notes
- source URLs
- thumbnail regeneration
- color reanalysis

## Required Eager UI Behavior
For direct, obvious consequences:

- trashing/removing from current scope should remove the entity from the visible grid immediately
- restoring/activating into the current visible scope should insert/update visible membership immediately where locally knowable
- rating/name/notes/source URLs should update visible inspector/grid metadata immediately

Do not force the controller to predict every derived smart-folder consequence. Those reconcile from backend state changes.

## Look For Adjacent Improvements
- collapse duplicated entity metadata cache handling
- remove file/entity naming ambiguity where the code clearly means “entity”
- simplify selection-based variants if they are only wrappers around one clearer controller method
- collapse near-duplicate file/entity calls that are the same action with slightly different names or input shapes
- remove redundant domain repetition like `setFileName` or `deleteFile` if `filesController.setName` or `filesController.delete` is clearer

## Non-Goals
- tag management
- folder membership
- collection membership

## Acceptance Criteria
1. All file/entity reads and writes route through `filesController`.
2. No feature-local backend access remains for this slice.
3. Direct visible consequences update eagerly.
4. Undo/redo for file/entity actions is controller-owned if PBI-559 is included or complete.

## Validation
- focused controller tests
- manual check from grid, inspector, viewer, and context menu
- manual check for status changes across scopes
