# Frontend Topology

This document defines the canonical `src/` directory structure and placement rules.

## Target Structure

```text
src/
  app/                  # App shell: composition root, bootstrap wiring, App.tsx
  entrypoints/          # Window entry points: main.tsx, detail.tsx, settings.tsx, etc.
  features/             # Domain-owned UI, hooks, controllers, and local helpers
    grid/               # Image grid, canvas, detail view, selection, layout
    sidebar/            # Sidebar navigation, folder tree, section items
    folders/            # Folder CRUD, folder picker, color/icon pickers
    tags/               # Tag manager, tag editor, tag select panel
    subscriptions/      # Flows, subscriptions, gallery-dl runner UI
    settings/           # Settings panels, PTR panel, developer panel
    collections/        # Collection views
    duplicates/         # Duplicate manager
    viewer/             # Detail window, slideshow, video player
    layout/             # Main view router, window controls
    inspector/          # Inspector state hook
    smart-folders/      # Smart folder modal, rule editor, predicate types
    app/                # Command palette
  shared/               # Cross-domain primitives only
    components/         # Reusable UI: buttons, tooltips, context menu, star rating, etc.
    hooks/              # (future) Shared hooks not owned by one feature
    lib/                # Formatters, shortcuts, drag helpers, notifications, media URLs
    styles/             # globals.css, renameInput, glassModal
    types/              # API types, generated command types, runtime contract types
    services/           # Folder picker service, tag picker service + portals
    contexts/           # Scoped display context
  state/                # App-global Zustand stores (navigation, settings, cache, domain, etc.)
  platform/             # Electron/desktop bridge: api.ts, global.d.ts
  runtime/              # Runtime resource invalidation and refresh orchestration
  controllers/          # Command dispatchers (transitional — to be split into features)
  hooks/                # Feature hooks (transitional — to be split into features)
  domain/               # Domain actions (transitional — to be split into features)
  components/           # Legacy component tree (transitional — being migrated to features/)
  test/                 # Test setup
```

## Placement Rules

1. **`src/` root** may contain only `vite-env.d.ts` and top-level directories. No `.tsx` files.
2. **Domain UI or domain logic** must not live directly in `src/` root.
3. **`src/features/*`** owns domain-specific components, hooks, controllers, and local helpers.
4. **`src/shared/*`** contains only genuinely cross-domain code. Nothing feature-specific.
5. **`src/platform/*`** contains Electron/desktop bridge code. Not mixed into shared utilities.
6. **`src/state/*`** contains app-global Zustand stores, not feature-local state.
7. **`src/app/*`** contains the app shell, composition root, and bootstrap wiring.
8. **`src/entrypoints/*`** contains window-level entry points loaded by HTML files.

## Import Aliases

| Alias | Resolves to | Purpose |
|-------|-------------|---------|
| `#desktop/*` | `src/platform/*` | Electron/desktop bridge |
| `#features/*` | `src/features/*` | Feature barrel re-exports |
| `#ui/*` | `src/shared/components/*` | Shared UI primitives |

## Migration Status

### Completed
- Root entry files moved to `src/entrypoints/`
- App shell moved to `src/app/`
- Desktop bridge moved to `src/platform/` (via alias)
- Stores moved to `src/state/`
- Shared code moved to `src/shared/` (lib, styles, types, services, contexts)
- UI primitives moved to `src/shared/components/`

### Transitional (to be completed in PBI-404/405)
- `src/components/` — legacy component tree, feature-owned items to be moved into `src/features/`
- `src/controllers/` — to be split between features
- `src/hooks/` — to be split between features and shared
- `src/domain/` — to be moved into features

## Classification Table

| Current Path | Classification | Target |
|-------------|---------------|--------|
| `src/components/image-grid/` | feature-owned | `src/features/grid/` |
| `src/components/sidebar/` | feature-owned | `src/features/sidebar/` |
| `src/components/smart-folders/` | feature-owned | `src/features/smart-folders/` |
| `src/components/settings/` | feature-owned | `src/features/settings/` |
| `src/components/subscriptions/` | feature-owned | `src/features/subscriptions/` |
| `src/components/tags/` | feature-owned | `src/features/tags/` |
| `src/components/video/` | feature-owned | `src/features/viewer/` |
| `src/components/layout/` | feature-owned | `src/features/layout/` |
| `src/components/dialogs/` | feature-owned | `src/features/grid/` |
| `src/components/Collections.tsx` | feature-owned | `src/features/collections/` |
| `src/components/DuplicateManager.tsx` | feature-owned | `src/features/duplicates/` |
| `src/components/FlowsWorking.tsx` | feature-owned | `src/features/subscriptions/` |
| `src/components/Settings.tsx` | feature-owned | `src/features/settings/` |
| `src/components/Slideshow.tsx` | feature-owned | `src/features/viewer/` |
| `src/components/TagManager.tsx` | feature-owned | `src/features/tags/` |
| `src/components/TagRelationsModal.tsx` | feature-owned | `src/features/tags/` |
| `src/components/TagChips.tsx` | feature-owned | `src/features/tags/` |
| `src/components/CommandPalette.tsx` | feature-owned | `src/features/app/` |
| `src/components/ZoomableImage.tsx` | feature-owned | `src/features/viewer/` |
| `src/components/AppErrorBoundary.tsx` | shared | `src/shared/components/` |
| `src/controllers/fileController.ts` | shared | `src/shared/controllers/` |
| `src/controllers/undoRedoController.ts` | shared | `src/shared/controllers/` |
| `src/controllers/gridController.ts` | feature-owned | `src/features/grid/` |
| `src/controllers/sidebarController.ts` | feature-owned | `src/features/sidebar/` |
| `src/controllers/folderController.ts` | feature-owned | `src/features/folders/` |
| `src/controllers/selectionController.ts` | feature-owned | `src/features/grid/` |
| `src/controllers/smartFolderController.ts` | feature-owned | `src/features/smart-folders/` |
| `src/controllers/subscriptionController.ts` | feature-owned | `src/features/subscriptions/` |
| `src/controllers/viewPrefsController.ts` | feature-owned | `src/features/grid/` |
| `src/controllers/perfController.ts` | shared | `src/shared/controllers/` |
| `src/controllers/ptrSyncController.ts` | feature-owned | `src/features/settings/` |
| `src/hooks/useInspectorData.ts` | feature-owned | `src/features/inspector/` |
| `src/hooks/useTagEditor.ts` | feature-owned | `src/features/tags/` |
| `src/hooks/useGridFeatureState.ts` | feature-owned | `src/features/grid/` |
| `src/hooks/useScopedGridPreferences.ts` | feature-owned | `src/features/grid/` |
| `src/hooks/useBoundaryNavigation.ts` | shared | `src/shared/hooks/` |
| `src/hooks/useDebouncedCallback.ts` | shared | `src/shared/hooks/` |
| `src/hooks/useGlobalKeydown.ts` | shared | `src/shared/hooks/` |
| `src/hooks/useGlobalPointerDrag.ts` | shared | `src/shared/hooks/` |
| `src/hooks/useInlineRename.ts` | shared | `src/shared/hooks/` |
| `src/domain/actions/` | feature-owned | `src/features/grid/actions/` |
