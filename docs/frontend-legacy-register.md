# Frontend Legacy Register

File-level classification of all non-canonical frontend code under `src/`.
Every entry has a target location and explicit delete condition.

## Classification Key

| Tag | Meaning |
|-----|---------|
| `canonical` | Final location, no action needed |
| `transitional` | Working code in legacy location, blocked on feature migration |
| `legacy-merge` | Move to canonical location, then delete original |

---

## Canonical Directories (complete, no action needed)

- `src/app/` — App.tsx, App.module.css, useAppBootstrap.ts
- `src/entrypoints/` — main.tsx, detail.tsx, settings.tsx, subscriptions.tsx, library-manager.tsx
- `src/platform/` — api.ts, global.d.ts
- `src/state/` — 8 Zustand stores
- `src/runtime/` — refresherOrchestrator.ts, resourceInvalidator.ts, 3 refreshers
- `src/shared/` — components/, contexts/, hooks/, lib/, services/, styles/, types/
- `src/features/` — barrel re-exports (will become real feature code after migration)
- `src/test/` — setup.ts

---

## src/hooks/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `useBoundaryNavigation.ts` | legacy-merge | `src/shared/hooks/` | move file, update 2 consumers, delete original |
| `useDebouncedCallback.ts` | legacy-merge | `src/shared/hooks/` | move file, update 2 consumers, delete original |
| `useGlobalKeydown.ts` | legacy-merge | `src/shared/hooks/` | move file, update 12 consumers, delete original |
| `useGlobalPointerDrag.ts` | legacy-merge | `src/shared/hooks/` | move file, update 3 consumers, delete original |
| `useInlineRename.ts` | legacy-merge | `src/shared/hooks/` | move file, update 4 consumers, delete original |
| `useInspectorData.ts` | transitional | `src/features/inspector/` | move when inspector feature gets real code |
| `useTagEditor.ts` | transitional | `src/features/tags/` | move when tags feature gets real code |
| `useGridFeatureState.ts` | transitional | `src/features/grid/` | move when grid feature gets real code |
| `useScopedGridPreferences.ts` | transitional | `src/features/grid/` | move when grid feature gets real code |

## src/domain/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `actions/fileLifecycleActions.ts` | transitional | `src/features/grid/actions/` | move when grid feature migration happens |
| `actions/mutationEffects.ts` | transitional | `src/features/grid/actions/` | move when grid feature migration happens |

## src/controllers/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `fileController.ts` | canonical | `src/shared/controllers/` | **DONE** — moved, 9 consumers updated |
| `undoRedoController.ts` | canonical | `src/shared/controllers/` | **DONE** — moved, 15 consumers updated |
| `perfController.ts` | canonical | `src/shared/controllers/` | **DONE** — moved, 1 consumer updated |
| `gridController.ts` | transitional | `src/features/grid/` | move when grid feature gets real code |
| `sidebarController.ts` | transitional | `src/features/sidebar/` | move when sidebar feature gets real code |
| `folderController.ts` | transitional | `src/features/folders/` | move when folder feature gets real code |
| `selectionController.ts` | transitional | `src/features/grid/` | move when grid feature gets real code |
| `smartFolderController.ts` | transitional | `src/features/smart-folders/` | move when smart-folder feature gets real code |
| `subscriptionController.ts` | transitional | `src/features/subscriptions/` | move when subscription feature gets real code |
| `viewPrefsController.ts` | transitional | `src/features/grid/` | move when grid feature gets real code |
| `ptrSyncController.ts` | transitional | `src/features/settings/` | move when settings feature gets real code |

---

## src/components/ — top-level files

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `AppErrorBoundary.tsx` | legacy-merge | `src/shared/components/` | move file, update 2 consumers (main.tsx, layout barrel), delete original |
| `Collections.tsx` | transitional | `src/features/collections/` | move when collections feature gets real code |
| `Collections.module.css` | transitional | moves with Collections.tsx | same as above |
| `CommandPalette.tsx` | transitional | `src/features/app/` | move when app feature gets real code |
| `CommandPalette.module.css` | transitional | moves with CommandPalette.tsx | same as above |
| `DuplicateManager.tsx` | transitional | `src/features/duplicates/` | move when duplicates feature gets real code |
| `DuplicateManager.module.css` | transitional | moves with DuplicateManager.tsx | same as above |
| `FlowsWorking.tsx` | transitional | `src/features/subscriptions/` | move when subscriptions feature gets real code |
| `FlowsWorking.module.css` | transitional | moves with FlowsWorking.tsx | same as above |
| `Settings.tsx` | transitional | `src/features/settings/` | move when settings feature gets real code |
| `Settings.module.css` | transitional | moves with Settings.tsx | same as above |
| `Slideshow.tsx` | transitional | `src/features/viewer/` | move when viewer feature gets real code |
| `Slideshow.module.css` | transitional | moves with Slideshow.tsx | same as above |
| `TagChips.tsx` | transitional | `src/features/tags/` | move when tags feature gets real code |
| `TagManager.tsx` | transitional | `src/features/tags/` | move when tags feature gets real code |
| `TagManager.module.css` | transitional | moves with TagManager.tsx | same as above |
| `TagRelationsModal.tsx` | transitional | `src/features/tags/` | move when tags feature gets real code |
| `TagRelationsModal.module.css` | transitional | moves with TagRelationsModal.tsx | same as above |
| `ZoomableImage.tsx` | transitional | `src/features/viewer/` | move when viewer feature gets real code |

## src/components/dialogs/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `BatchRenameDialog.tsx` | transitional | `src/features/grid/` | move when grid feature migration happens |
| `BatchRenameDialog.module.css` | transitional | moves with dialog | same as above |

## src/components/layout/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `MainViewRouter.tsx` | transitional | `src/features/layout/` | move when layout feature gets real code |
| `MainViewModelContext.tsx` | transitional | `src/features/layout/` | same as above |
| `SidebarJobStatus.tsx` | transitional | `src/features/layout/` | same as above |
| `SidebarJobStatus.module.css` | transitional | moves with component | same as above |
| `WindowControls.tsx` | transitional | `src/features/layout/` | same as above |
| `WindowControls.module.css` | transitional | moves with component | same as above |

## src/components/settings/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `GeneralPanel.tsx` | transitional | `src/features/settings/` | move when settings feature gets real code |
| `LibraryPanel.tsx` | transitional | `src/features/settings/` | same |
| `DeveloperPanel.tsx` | transitional | `src/features/settings/` | same |
| `DangerZonePanel.tsx` | transitional | `src/features/settings/` | same |
| `DuplicatesPanel.tsx` | transitional | `src/features/settings/` | same |
| `PtrPanel.tsx` | transitional | `src/features/settings/` | same |
| `DownloadServicesPanel.tsx` | transitional | `src/features/settings/` | same |
| `ShortcutsPanel.tsx` | transitional | `src/features/settings/` | same |
| `ShortcutsPanel.module.css` | transitional | moves with component | same |
| `ui.tsx` | transitional | `src/features/settings/` | same |

## src/components/sidebar/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `Sidebar.tsx` | transitional | `src/features/sidebar/` | move when sidebar feature gets real code |
| `Sidebar.module.css` | transitional | moves with component | same |
| `SidebarItem.tsx` | transitional | `src/features/sidebar/` | same |
| `SidebarSection.tsx` | transitional | `src/features/sidebar/` | same |
| `SidebarMenuButton.tsx` | transitional | `src/features/sidebar/` | same |
| `SidebarMenuButton.module.css` | transitional | moves with component | same |
| `FolderTree.tsx` | transitional | `src/features/sidebar/` | same |
| `SmartFolderList.tsx` | transitional | `src/features/sidebar/` | same |
| `LibrarySwitcher.tsx` | transitional | `src/features/sidebar/` | same |
| `LibrarySwitcher.module.css` | transitional | moves with component | same |
| `contextMenuRegistry.tsx` | transitional | `src/features/sidebar/` | same |

## src/components/smart-folders/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `SmartFolderModal.tsx` | transitional | `src/features/smart-folders/` | move when smart-folders feature gets real code |
| `RuleEditor.tsx` | transitional | `src/features/smart-folders/` | same |
| `RuleGroupEditor.tsx` | transitional | `src/features/smart-folders/` | same |
| `ValuePicker.tsx` | transitional | `src/features/smart-folders/` | same |
| `TagPickerMenu.tsx` | transitional | `src/features/smart-folders/` | same |
| `IconPicker.tsx` | transitional | `src/features/smart-folders/` | same |
| `FolderIconPicker.tsx` | transitional | `src/features/smart-folders/` | same |
| `FolderColorPicker.tsx` | transitional | `src/features/smart-folders/` | same |
| `iconRegistry.tsx` | transitional | `src/features/smart-folders/` | same |
| `fieldConfig.ts` | transitional | `src/features/smart-folders/` | same |
| `types.ts` | transitional | `src/features/smart-folders/` | same |

## src/components/subscriptions/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `SubscriptionsWindow.tsx` | transitional | `src/features/subscriptions/` | move when subscriptions feature gets real code |
| `SubscriptionsWindow.module.css` | transitional | moves with component | same |

## src/components/tags/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `TagSelectPanel.tsx` | transitional | `src/features/tags/` | move when tags feature gets real code |
| `TagSelectPanel.module.css` | transitional | moves with component | same |
| `tagSelectService.ts` | transitional | `src/features/tags/` | same |
| `tagSelectTypes.ts` | transitional | `src/features/tags/` | same |

## src/components/video/

| File | Classification | Target | Delete condition |
|------|---------------|--------|-----------------|
| `VideoPlayer.tsx` | transitional | `src/features/viewer/` | move when viewer feature gets real code |
| `VideoPlayer.module.css` | transitional | moves with component | same |
| `VideoControls.tsx` | transitional | `src/features/viewer/` | same |
| `ProgressBar.tsx` | transitional | `src/features/viewer/` | same |
| `PlaybackRateMenu.tsx` | transitional | `src/features/viewer/` | same |
| `VolumePanel.tsx` | transitional | `src/features/viewer/` | same |
| `VolumeHUD.tsx` | transitional | `src/features/viewer/` | same |
| `useVideoPlayer.ts` | transitional | `src/features/viewer/` | same |
| `useFrameTime.ts` | transitional | `src/features/viewer/` | same |
| `videoConstants.ts` | transitional | `src/features/viewer/` | same |
| `videoTimeFormat.ts` | transitional | `src/features/viewer/` | same |

## src/components/image-grid/ (blocked on PBI-408 split)

All 44 source files + 10 hooks + 4 queryBroker + 7 runtime + 2 viewer files are transitional.
Target: `src/features/grid/` after PBI-408 architectural split.
Delete condition: split image-grid into rendering, runtime, hooks, queries, media, and viewer subsystems, then move each into `src/features/grid/`.

| Subsystem | Files | Target subdirectory |
|-----------|-------|-------------------|
| Core rendering | CanvasGrid, VirtualGrid, LayoutRow, ImageGrid, layoutMath, layoutWorker, DragGhost, SortByRow | `src/features/grid/rendering/` |
| Grid controls | ImageGridControls, FilterBar, DisplayOptionsPanel, SubfolderGrid | `src/features/grid/controls/` |
| Properties/inspector | ImagePropertiesPanel, DetailView | `src/features/grid/inspector/` |
| Viewer | DetailWindow, QuickLook, StripView, GlassImagePreview, VideoScrubOverlay, Slideshow, ZoomableImage | `src/features/viewer/` |
| Media caching | imageAtlas, enhancedMediaCache, metadataPrefetch, mediaQosScheduler, blurhashDecodeWorker | `src/features/grid/media/` |
| Image hooks | useImageZoom, useImageLoadState, useImagePreloader, useNavigatorDrag, useNavigatorRenderer, useZoomCache | `src/features/grid/media/` |
| Grid hooks | useGridContextMenu, useGridHotkeys, useGridItemActions, useGridKeyboardNavigation, useGridMarqueeSelection, useGridMutationActions, useGridScopeTransition, useGridSelection, useGridTransitionController, useWaterfallLayoutWorker | `src/features/grid/hooks/` |
| Query broker | GridQueryBroker, gridQueryKey, useGridQueryBroker, index | `src/features/grid/queryBroker/` |
| Runtime state | gridRuntimeReducer, gridRuntimeSelectors, gridRuntimeState, gridTransitionPipeline, gridViewerSession, useGridRuntime, index | `src/features/grid/runtime/` |
| Viewer media | preloadPlan, useViewerMediaPipeline | `src/features/viewer/media/` |
| Shared types | shared.ts | `src/features/grid/types.ts` |
| Styles | 11 .module.css files | move with owning component |

---

## Deletion Priority

### Tier 1: Move now (shared utilities, no feature dependency)

5 shared hooks → `src/shared/hooks/` + 1 component → `src/shared/components/`.
Total: 6 moves, ~21 consumer import updates.

### Tier 2: Merge shared controllers

3 shared controllers → `src/shared/controllers/`.
Total: 3 moves + consumer import updates.

### Tier 3: Feature migration (PBI-405)

Move components from `src/components/<domain>/` into `src/features/<domain>/`.
Update barrel re-exports to point at local code. Delete `src/components/<domain>/`.

### Tier 4: Architectural split (PBI-408)

`src/components/image-grid/` (65+ files) needs decomposition before migration.
Split by subsystem (see table above), then move into `src/features/grid/`.
