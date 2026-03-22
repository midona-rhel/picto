# PBI-559: Move Undo/Redo Ownership Into Controllers

## AI-Generated Caveat
This PBI is AI-generated and intentionally cross-cuts multiple domains because the architectural problem is ownership, not domain logic. The engineer should still migrate undo/redo one coherent slice at a time inside this PBI if needed.

## Priority
P0

## Problem
Undo/redo registration is still scattered through UI hooks and components. That means:

- normal action paths and undo paths diverge
- controller guarantees are weak
- the same action can behave differently from different surfaces

## Goal
Make controllers own undo/redo registration for migrated domains. UI code should stop calling `registerUndoAction(...)` for domain actions.

## Atomicity Rule
This PBI is complete only when controller ownership is the rule, not the exception, for the migrated domains. Do not stop halfway with a mixed ownership model.

## Scope

### Core utility
- [src/shared/controllers/undoRedoController.ts](./src/shared/controllers/undoRedoController.ts)
- [src/state/undoRedoStore.ts](./src/state/undoRedoStore.ts)

### Known UI-owned undo callers to eliminate
- [src/features/grid/hooks/useGridStateActions.ts](./src/features/grid/hooks/useGridStateActions.ts)
- [src/features/grid/hooks/useGridInlineRename.ts](./src/features/grid/hooks/useGridInlineRename.ts)
- [src/features/grid/hooks/useGridItemActions.ts](./src/features/grid/hooks/useGridItemActions.ts)
- [src/features/inspector/hooks/useInspectorState.ts](./src/features/inspector/hooks/useInspectorState.ts)
- [src/features/inspector/hooks/useInspectorChangeActions.ts](./src/features/inspector/hooks/useInspectorChangeActions.ts)
- [src/features/tags/components/TagManager.tsx](./src/features/tags/components/TagManager.tsx)
- [src/features/sidebar/hooks/useFolderTreeActions.ts](./src/features/sidebar/hooks/useFolderTreeActions.ts)
- [src/features/sidebar/hooks/useFolderTreeDnd.ts](./src/features/sidebar/hooks/useFolderTreeDnd.ts)
- [src/features/sidebar/components/SmartFolderList.tsx](./src/features/sidebar/components/SmartFolderList.tsx)
- [src/shared/components/context-actions/imageActions.tsx](./src/shared/components/context-actions/imageActions.tsx)

## Required Outcome
Controllers own:

- normal action
- undo registration
- redo registration

UI owns:

- invoking the controller
- rendering current state

## Look For Adjacent Improvements
- unify inverse-operation helpers that are only used for undo
- simplify controller method shapes so undo can call the same public method
- remove stale “no undo” comments that are no longer true
- collapse near-duplicate action variants first, so undo/redo targets one canonical controller call instead of many wrappers
- normalize controller method naming first so undo/redo binds to clear canonical action names

## Acceptance Criteria
1. UI files no longer register undo actions for migrated domain operations.
2. Controllers register undo/redo using the same public operation shape.
3. Undo/redo from all relevant surfaces behaves consistently.

## Validation
- undo/redo status changes
- undo/redo tag changes
- undo/redo folder/collection membership changes
- undo/redo folder tree and smart-folder tree actions
