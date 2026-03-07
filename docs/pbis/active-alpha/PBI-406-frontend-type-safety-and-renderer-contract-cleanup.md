# PBI-406: Frontend type safety and renderer contract cleanup

## Priority
P1

## Audit Status (2026-03-08)
Status: **Implemented**

Implementation:
1. Added `SmartFolderIpcInput` and `DragDropPayload` types to `src/shared/types/api/core.ts`.
2. Removed all 6 `as any` casts from `SmartFolderModal.tsx` — `predicateToRust`/`folderToRust` now return typed `SmartFolderPredicate`/`SmartFolderIpcInput`.
3. Removed `as any` from `ImageGrid.tsx` drag-drop handler — uses explicit `DragDropPayload` type.
4. Removed `as any` from `folderController.ts` — uses `FolderReorderMove[]` (aligned with generated type).
5. Fixed stale `FolderReorderMove` in `core.ts` to match generated Rust type (`before_hash`/`after_hash` instead of `position_rank`).
6. Removed `as any` from `api.ts` `onDragDropEvent` handler and `ContextMenu.tsx` item narrowing.
7. Removed `as any` from `smartFolderController.ts` — accepts `SmartFolderIpcInput` directly.
8. Replaced `ComponentType<any>` in `EmptyState`, `StateBlock`, and `FilterBar` with `TablerIcon` import.
9. Fixed `App.tsx` `sf.predicate as any` — typed `SmartFolderSummary.predicate` as `SmartFolderPredicate` in `domainStore.ts`.
10. Fixed `CommandPalette.tsx` union-array push casts — properly typed `PaletteRow[]` union.
11. Replaced `any[]` in `useDebouncedCallback` with proper generic `A extends unknown[]` pattern.
12. Remaining unavoidable escapes (7 total): V8 GC API (`cacheCleanup.ts` x2), Mantine Tooltip children (`KbdTooltip.tsx`), CSS variable in CSSProperties (`DragGhost.tsx`), test mocks (`setup.ts`, `gridMarqueeSelection.test.ts` x2).
13. Generated type `PredicateRule.ts` carries `value: any, value2: any` — acceptable-by-design since the Rust source type is `serde_json::Value` (heterogeneous union). Tightening requires upstream schema change.
14. `npx tsc --noEmit` passes clean.

## Problem
TypeScript coverage is no longer the main compile blocker, but the renderer still relies on too many escape hatches at the exact boundaries where regressions are expensive: IPC payloads, smart-folder predicates, drag/drop events, and context-menu state.

## Scope
- `src/desktop/api.ts`
- `src/components/smart-folders/`
- `src/components/image-grid/`
- `src/controllers/folderController.ts`
- Shared UI helpers still typed with `any`

## Implementation
1. Replace repeated `any`/`as any` payload casts with explicit interfaces.
2. Introduce typed wrappers for Electron/webview drag-drop payloads.
3. Tighten smart-folder create/update/count request and response types.
4. Remove avoidable `ComponentType<any>` usage in shared UI primitives.
5. Keep unavoidable platform-specific typing gaps isolated behind one typed adapter layer.

## Acceptance Criteria
1. Runtime/domain code no longer uses casual `any` casts where a stable type can be defined.
2. Smart-folder and drag/drop flows compile against explicit request/response types.
3. Folder reorder and context-menu helper payloads no longer rely on `as any`.
4. The remaining unavoidable type escapes are documented and isolated.

## Test Cases
1. `npx tsc -p tsconfig.json --noEmit` passes.
2. Smart-folder create/update/count flows still work.
3. Grid drag/drop and sidebar drag/drop still work.

## Risk
Medium. The work is mostly type-level, but it touches mutation and IPC boundaries that are easy to break if refactored casually.
