# PBI-564: Eliminate Remaining UI-Owned Undo/Redo Registrations

## AI-Generated Caveat
This PBI is AI-generated from a live code scan on 2026-03-22. It is intentionally an audit-and-finish slice. The exact caller list may shrink while earlier PBIs land, so the implementing engineer should update the list as part of the same change rather than preserving stale targets.

## Priority
P0

## Problem
Undo/redo ownership has improved, but many renderer hooks and components still call `registerUndoAction(...)` directly. That means the app still has two behavior paths:

- the normal controller path
- a separate UI-owned undo/redo path

That weakens eager frontend updates, state-change reconciliation, task gating, and consistency across surfaces.

## Goal
Finish the undo/redo migration by removing the remaining UI-owned `registerUndoAction(...)` calls and moving those reversible behaviors into the correct controllers.

## Atomicity Rule
This PBI is atomic around undo/redo ownership only. Do not reopen raw backend boundary work or unrelated state-change contract work unless a tiny helper is required to finish the ownership migration cleanly.

## Relationship To PBI-559
This PBI is a concrete finish pass for the broad architectural goal in [PBI-559: Move Undo/Redo Ownership Into Controllers](./PBI-559-move-undo-redo-ownership-into-controllers.md).

Use this PBI when the remaining work is no longer “design the rule,” but “remove the leftover UI-owned callers and finish the migration.”

## Scope

### Core utility
- [src/shared/controllers/undoRedoController.ts](./src/shared/controllers/undoRedoController.ts)
- [src/state/undoRedoStore.ts](./src/state/undoRedoStore.ts)

### Remaining UI-owned undo callers to eliminate
- [src/features/settings/components/DuplicatesPanel.tsx](./src/features/settings/components/DuplicatesPanel.tsx)
- [src/features/grid/hooks/useGridItemActions.ts](./src/features/grid/hooks/useGridItemActions.ts)
- [src/features/grid/hooks/useGridInlineRename.ts](./src/features/grid/hooks/useGridInlineRename.ts)
- [src/features/grid/hooks/useGridStateActions.ts](./src/features/grid/hooks/useGridStateActions.ts)
- [src/features/smart-folders/components/SmartFolderModal.tsx](./src/features/smart-folders/components/SmartFolderModal.tsx)
- [src/features/duplicates/components/DuplicateManager.tsx](./src/features/duplicates/components/DuplicateManager.tsx)
- [src/features/sidebar/components/SmartFolderList.tsx](./src/features/sidebar/components/SmartFolderList.tsx)
- [src/features/sidebar/components/Sidebar.tsx](./src/features/sidebar/components/Sidebar.tsx)
- [src/features/sidebar/hooks/useFolderTreeActions.ts](./src/features/sidebar/hooks/useFolderTreeActions.ts)
- [src/features/sidebar/hooks/useFolderTreeDnd.ts](./src/features/sidebar/hooks/useFolderTreeDnd.ts)
- [src/features/tags/components/TagManager.tsx](./src/features/tags/components/TagManager.tsx)
- [src/features/inspector/hooks/useInspectorState.ts](./src/features/inspector/hooks/useInspectorState.ts)
- [src/features/inspector/hooks/useInspectorChangeActions.ts](./src/features/inspector/hooks/useInspectorChangeActions.ts)
- [src/shared/components/BatchRenameDialog.tsx](./src/shared/components/BatchRenameDialog.tsx)
- [src/shared/components/context-actions/imageActions.tsx](./src/shared/components/context-actions/imageActions.tsx)

### Controllers likely to absorb the remaining ownership
- [src/controllers/filesController.ts](./src/controllers/filesController.ts)
- [src/controllers/tagsController.ts](./src/controllers/tagsController.ts)
- [src/controllers/foldersController.ts](./src/controllers/foldersController.ts)
- [src/controllers/collectionsController.ts](./src/controllers/collectionsController.ts)
- [src/controllers/smartFoldersController.ts](./src/controllers/smartFoldersController.ts)
- [src/controllers/duplicatesController.ts](./src/controllers/duplicatesController.ts)
- [src/controllers/sidebarController.ts](./src/controllers/sidebarController.ts)

## Required Outcome
- UI code stops calling `registerUndoAction(...)` for migrated domain actions.
- Controllers expose one canonical action path for:
  - user action
  - undo
  - redo
- Undo/redo uses the same controller surface as normal actions, with eager frontend updates and state-change reconciliation.
- Controller methods suppress re-registering undo during undo/redo execution through one explicit mechanism such as:
  - `recordUndo: boolean`
  - `source: 'user' | 'undo' | 'redo'`
  - or another equally clear execution mode

## Look For Adjacent Improvements
- collapse near-duplicate controller methods before binding undo/redo to them
- remove inverse-operation helpers that exist only because UI used to own undo
- rename controller methods so undo/redo targets clear canonical operations
- delete stale comments like “no undo” or “UI handles undo here”
- simplify multi-surface actions so grid, inspector, sidebar, and context menus all call the same controller method

## Non-Goals
- redesigning the undo stack data structure
- changing keyboard/menu bindings for undo/redo
- broad backend event-contract rewrites

## Acceptance Criteria
1. No migrated UI file calls `registerUndoAction(...)` directly.
2. Controllers register undo/redo for the migrated actions.
3. Undo and redo call the same public controller operations used by normal actions, with explicit suppression of recursive undo registration.
4. Eager frontend updates and state-change reconciliation behave the same for normal action, undo, and redo.

## Validation
- focused scan for remaining `registerUndoAction(...)` call sites
- undo/redo from grid, inspector, sidebar, and context menu match normal action behavior
- undo/redo still respects task blocking where relevant
- no duplicate undo entries are created during redo or undo execution
