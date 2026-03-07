# Frontend Audit Status

Date: 2026-03-07

This report completes the work in former `PBI-241`.

## Scope Reviewed

Reviewed frontend and Electron surface area across 320 files:

| Area | Files | Status | Summary |
| --- | ---: | --- | --- |
| `src/App.tsx` and root entrypoints | 6 | Reviewed | App shell ownership is still too wide; entrypoints are thin but shell state remains centralized in `App.tsx`. |
| `src/app-shell/` | 1 | Reviewed | Bootstrap owns startup, menu listeners, theme, event bridge wiring, and task runtime initialization in one hook. |
| `src/components/` | 203 | Reviewed | Largest source of ownership drift; legacy component surfaces still coexist with feature exports. |
| `src/features/` | 46 | Reviewed | Feature-first structure exists, but migration is incomplete and mixed with direct legacy imports. |
| `src/stores/` | 13 | Reviewed | Zustand usage is generally coherent, but some lifecycle/state ownership still leaks back into components and hooks. |
| `src/hooks/` | 10 | Reviewed | Several hooks have grown into orchestration layers and rely on dependency suppression. |
| `src/controllers/` | 12 | Reviewed | Controller layer exists, but not all flows are controller-owned yet. |
| `src/services/` | 5 | Reviewed | Small surface area, but portal services are still ad hoc and outside feature boundaries. |
| `src/contexts/` | 1 | Reviewed | Limited scope; no material issues beyond surrounding ownership drift. |
| `src/lib/` | 11 | Reviewed | Mostly focused helpers, with a few debug/logging leftovers. |
| `src/utils/` | 1 | Reviewed | Minimal surface area. |
| `src/types/` | 5 | Reviewed | Core API typing is good overall, but runtime escapes remain around `any` and loosely typed contracts. |
| `src/styles/` | 8 | Reviewed | Styling approach is mixed but not currently a release blocker. |
| `src/desktop/` | 2 | Reviewed | Renderer transport layer is oversized and mixes unrelated responsibilities. |
| `electron/` | 11 | Reviewed | Main-process structure is acceptable, but transport/domain boundaries can still be tightened. |

## Hotspots

Largest files reviewed:

1. `src/components/image-grid/CanvasGrid.tsx` — 2321 lines
2. `src/components/image-grid/ImageGrid.tsx` — 1181 lines
3. `src/components/image-grid/VirtualGrid.tsx` — 952 lines
4. `src/components/image-grid/imageAtlas.ts` — 860 lines
5. `src/components/sidebar/FolderTree.tsx` — 827 lines
6. `src/components/FlowsWorking.tsx` — 822 lines
7. `src/components/tags/TagSelectPanel.tsx` — 809 lines
8. `src/components/ui/context-actions/imageActions.tsx` — 799 lines
9. `src/components/TagManager.tsx` — 785 lines
10. `src/desktop/api.ts` — 770 lines
11. `src/hooks/useInspectorData.ts` — 710 lines

These are the main sources of responsibility drift and cleanup debt.

## Findings

### 1. Feature-first migration is incomplete

Evidence:

1. `src/App.tsx` imports both feature exports (`#features/grid/components`, `#features/layout/components`, `#features/sidebar/components`) and legacy direct component paths (`./components/ui/KbdTooltip`, `./services/TagPickerPortal`, `./services/FolderPickerPortal`).
2. Legacy heavy modules remain under `src/components/` even where feature folders already exist:
   - `src/components/FlowsWorking.tsx`
   - `src/components/TagManager.tsx`
   - `src/components/Collections.tsx`
   - `src/components/DuplicateManager.tsx`
3. The project currently has both `src/components/sidebar/` and `src/features/sidebar/`, both `src/components/settings/` and `src/features/settings/`, and similar dual surfaces in several domains.

Impact:

1. Ownership is hard to infer from file location.
2. New work can keep landing in the legacy tree because the migration boundary is not enforced.
3. Audit/review cost stays high because imports do not communicate domain ownership cleanly.

Backlog mapping:

1. New `PBI-401`

### 2. Type safety still has too many runtime escape hatches

Evidence from current runtime files:

1. `src/controllers/folderController.ts` uses `moves as any` for reorder payloads.
2. `src/App.tsx` casts smart-folder predicates with `predicate as any`.
3. `src/components/smart-folders/SmartFolderModal.tsx` uses repeated `as any` casts for create/update/count payloads and responses.
4. `src/components/CommandPalette.tsx`, `src/components/ui/ContextMenu.tsx`, `src/components/ui/KbdTooltip.tsx`, `src/components/image-grid/DragGhost.tsx`, and `src/desktop/api.ts` still rely on `any`-typed bridging.
5. `src/components/image-grid/ImageGrid.tsx` handles webview drag payloads through `event.payload as any`.
6. `src/components/ui/EmptyState.tsx` and `src/components/ui/state/StateBlock.tsx` still expose `ComponentType<any>`.

Impact:

1. Renderer/runtime contract changes are harder to verify by TypeScript alone.
2. Smart-folder and context-menu changes are more fragile than they need to be.
3. Transport layer bugs can remain invisible until runtime.

Backlog mapping:

1. New `PBI-402`
2. Existing `PBI-234` remains the broader typed dispatch contract item, but it does not cover these renderer-side cleanup tasks specifically.

### 3. Lifecycle and effect ownership are still split across too many layers

Evidence:

1. `src/app-shell/useAppBootstrap.ts` owns settings init, theme sync, window show, menu listeners, event bridge setup, task runtime init, and titlebar drag behavior.
2. There are multiple `react-hooks/exhaustive-deps` suppressions in:
   - `src/app-shell/useAppBootstrap.ts`
   - `src/components/image-grid/DetailView.tsx`
   - `src/components/video/VideoPlayer.tsx`
   - `src/components/video/useVideoPlayer.ts`
   - `src/hooks/useBoundaryNavigation.ts`
   - `src/components/Slideshow.tsx`
3. View transition ownership is still spread across `App.tsx`, `useAppBootstrap`, `ImageGrid.tsx`, and transition hooks.
4. Subscription runtime state is split between `useTaskRuntimeStore`, `setupEventBridge()`, and view components.

Impact:

1. Event ordering bugs are easy to introduce.
2. Strict Mode / remount behavior remains risky.
3. The code is harder to reason about than it needs to be for basic app lifecycle flows.

Backlog mapping:

1. Existing `PBI-244`
2. New `PBI-403`

### 4. Several modules are too large and mix orchestration with presentation

Evidence:

1. `src/components/FlowsWorking.tsx` mixes data loading, subscription lifecycle listeners, flow CRUD, query CRUD, inline rename, notifications, and modal state.
2. `src/components/sidebar/FolderTree.tsx` mixes tree building, DnD, inline rename, context menu policy, delete flows, sort operations, folder auto-tagging, and drop handling.
3. `src/hooks/useInspectorData.ts` owns fetch orchestration, optimistic edits, undo registration, notes autosave, folder membership, selection summary, and collection summary.
4. `src/desktop/api.ts` mixes transport wrappers, window helpers, dialog helpers, library host access, and local store implementation in one file.

Impact:

1. These files are difficult to review safely.
2. Testing tends to stay too coarse because responsibilities are not isolated.
3. Small behavior changes require touching high-risk files.

Backlog mapping:

1. Existing `PBI-229` absorbs the subscription panel/flow surface finding.
2. New `PBI-404`

### 5. Some current active PBIs needed concrete audit targets

Updated with audit-specific scope:

1. `PBI-229` now explicitly targets `src/components/FlowsWorking.tsx` and `src/components/subscriptions/SubscriptionsWindow.tsx`.
2. `PBI-244` now explicitly targets `src/App.tsx`, `src/app-shell/useAppBootstrap.ts`, `src/components/image-grid/ImageGrid.tsx`, and surrounding transition/bootstrap drift.

## New PBIs Created From This Audit

1. `PBI-401` — frontend surface consolidation and feature-first boundary enforcement
2. `PBI-402` — frontend type safety and renderer contract cleanup
3. `PBI-403` — renderer bootstrap and lifecycle ownership cleanup
4. `PBI-404` — split oversized frontend orchestration modules

## Conclusion

`PBI-241` is complete.

The frontend has now been audited at the directory/module level, and the remaining uncatalogued cleanup debt from that audit is tracked by:

1. Existing `PBI-229`
2. Existing `PBI-244`
3. New `PBI-401`
4. New `PBI-402`
5. New `PBI-403`
6. New `PBI-404`
