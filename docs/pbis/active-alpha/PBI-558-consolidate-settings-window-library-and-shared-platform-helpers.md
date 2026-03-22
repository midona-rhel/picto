# PBI-558: Consolidate Settings, Window, Library, And Shared Platform Helpers

## AI-Generated Caveat
This PBI is AI-generated and intentionally groups the remaining app-level backend access points that are not cleanly part of a content domain. If a smaller cut is clearly safer, split it, but keep the same end state.

## Priority
P1

## Problem
Settings access, window/open-external behavior, library reads/actions, and a few shared UI helpers still tend to bypass domain controllers because they are treated as “small platform calls.”

## Goal
Make these app/platform interactions consistent and controller-owned.

## Atomicity Rule
This PBI should finish this support slice only. Do not expand into subscriptions/files if their domain controllers already cover the behavior.

## Scope

### Controllers
- [src/controllers/settingsController.ts](./src/controllers/settingsController.ts)
- [src/controllers/windowController.ts](./src/controllers/windowController.ts)
- [src/controllers/libraryController.ts](./src/controllers/libraryController.ts)
- [src/controllers/sidebarController.ts](./src/controllers/sidebarController.ts)
- [src/controllers/companionController.ts](./src/controllers/companionController.ts)

### Consumers
- [src/shared/hooks/useScopedGridPreferences.ts](./src/shared/hooks/useScopedGridPreferences.ts)
- [src/state/domainStore.ts](./src/state/domainStore.ts)
- [src/state/libraryStore.ts](./src/state/libraryStore.ts)
- [src/shared/components/UrlListEditor.tsx](./src/shared/components/UrlListEditor.tsx)
- [src/shared/services/AiTaggerPortal.tsx](./src/shared/services/AiTaggerPortal.tsx)
- [src/app/App.tsx](./src/app/App.tsx)
- [src/app/useNativeEventListeners.ts](./src/app/useNativeEventListeners.ts)
- [src/entrypoints/library-manager.tsx](./src/entrypoints/library-manager.tsx)

## Required Outcome
- settings reads/writes route through `settingsController`
- window/open-external/open-settings/open-subscriptions route through `windowController` or platform helper wrappers
- library actions/info route through `libraryController`
- sidebar tree and companion reads route through dedicated controllers or one clearly named controller per concern

## Look For Adjacent Improvements
- remove app-level helper duplication
- simplify theme bootstrap reads
- collapse “small one-off helper” wrappers into a clearer controller if it improves discoverability
- collapse near-duplicate settings/window/library helper calls into canonical controller methods
- remove redundant names like `openSettingsWindow` if `windowController.openSettings` is clearer and equally specific
- move undo/redo registration for reversible library/sidebar actions into controllers when those flows are part of this slice

## Acceptance Criteria
1. No raw backend access remains in app/shared/entrypoint code in this slice.
2. Settings/view-prefs loading and saving still work.
3. Window/open-external actions still work.
4. Library manager still works.
5. Undo/redo is controller-owned for reversible actions in this slice if PBI-559 is complete.

## Validation
- scoped grid preferences read/write
- open external URL
- open settings / open subscriptions
- library info and manager actions
- undo/redo for any migrated reversible library/sidebar actions behaves the same from every surface
