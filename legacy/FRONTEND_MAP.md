# Legacy frontend surface map

The legacy frontend was moved from `src/` to `legacy/frontend/` as part of PBI-589.
It is reference-only — not part of the active build.

## Directory mapping

| Legacy path | Purpose |
|---|---|
| `legacy/frontend/entrypoints/` | Electron window entry points (main, detail, settings, subscriptions, library-manager) |
| `legacy/frontend/app/` | App shell (App.tsx, useAppBootstrap, useCommandPalette) |
| `legacy/frontend/features/grid/` | Grid screen (ImageGrid, GridRoot, CanvasGrid, runtime, hooks) |
| `legacy/frontend/features/sidebar/` | Sidebar (FolderTree, SmartFolderList, Sidebar) |
| `legacy/frontend/features/inspector/` | Inspector panel (InspectorPanel, useInspectorState) |
| `legacy/frontend/features/viewer/` | Media viewer (MediaView, DetailWindow, VideoPlayer) |
| `legacy/frontend/features/layout/` | Layout (MainViewRouter, FilterBar, MainViewModelContext) |
| `legacy/frontend/features/tags/` | Tag manager |
| `legacy/frontend/features/collections/` | Collections view |
| `legacy/frontend/features/subscriptions/` | Subscriptions panel |
| `legacy/frontend/features/duplicates/` | Duplicate manager |
| `legacy/frontend/features/smart-folders/` | Smart folder modal |
| `legacy/frontend/features/folders/` | Folder components (watch dialog, subfolder grid) |
| `legacy/frontend/platform/` | API layer (api.ts, targets.ts, ipc.ts) |
| `legacy/frontend/controllers/` | Domain controllers (entity, tags, folders, collections, etc.) |
| `legacy/frontend/state/` | Jotai atoms (sidebar, navigation, selection, filters, grid, settings, runtime) |
| `legacy/frontend/state-legacy/` | Old Zustand stores (domainStore, navigationStore, settingsStore, etc.) |
| `legacy/frontend/runtime/` | Backend event reconciliation (stateChanges, appliers) |
| `legacy/frontend/shared/` | Shared components, hooks, types, styles |
| `legacy/frontend/styles/` | Global CSS (globals.css, themes) |
| `legacy/frontend/test/` | Test setup |

## New src/ structure

The active `src/` tree is for the rebuilt frontend only.

| Path | Purpose |
|---|---|
| `src/entrypoints/` | Window entry points (placeholder until rebuild lands) |
| `src/platform/` | Frontend API layer and transport adapter |
| `src/controllers/` | Domain action boundaries |
| `src/state/` | Jotai-owned frontend state |
| `src/runtime/` | Backend reconciliation and refresh targeting |
| `src/features/` | Feature roots and composition |
| `src/shared/components/` | Presentational UI and reusable primitives |
| `src/shared/styles/` | Shared CSS |
| `src/styles/` | Global styles |
