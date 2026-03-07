# PBI-406: Frontend type safety and renderer contract cleanup

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. Runtime files still contain `any`/`as any` escapes in smart-folder payloads, command palette flattening, drag/drop payload handling, and folder reorder commands.
2. `src/components/image-grid/ImageGrid.tsx` still handles webview drag-drop payloads with `event.payload as any`.
3. `src/components/smart-folders/SmartFolderModal.tsx` uses repeated `as any` casts for API calls.
4. `src/desktop/api.ts` still exposes several loosely typed bridge helpers and handler casts.

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
