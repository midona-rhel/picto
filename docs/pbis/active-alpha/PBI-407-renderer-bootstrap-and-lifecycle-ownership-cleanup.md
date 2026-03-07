# PBI-407: Renderer bootstrap and lifecycle ownership cleanup

## Priority
P1

## Audit Status (2026-03-08)
Status: **Implemented**

Implementation:
1. `eventBridge.ts` and `taskRuntimeStore.ts` already deleted in PBI-402 — replaced by `runtimeSyncStore` + `refresherOrchestrator`.
2. Extracted `src/shared/hooks/useThemeSync.ts` — unified theme sync hook (settings init + Mantine color scheme + DOM attribute). Deduplicates identical pattern from `useAppBootstrap` and 3 entrypoints (`settings.tsx`, `subscriptions.tsx`, `library-manager.tsx`). Uses refs for Mantine's `setColorScheme`/`colorScheme` to avoid exhaustive-deps suppression.
3. Extracted `src/app/useNativeEventListeners.ts` — consolidates sidebar init, runtime sync init, refresher lifecycle, library switching/switched listeners, and menu event listeners (`open-settings`, `navigate`, `undo`, `redo`) from `useAppBootstrap`.
4. Fixed avoidable `[]` suppression in `useAppBootstrap` startup effect by adding stable `appWindow` dep (from `useMemo`).
5. Suppressions reduced from 21 → 16. Remaining 16 are all in image-grid/viewer modules (deferred to PBI-408): `DetailView.tsx` ×6, `useImageLoadState.ts` ×2, `useZoomCache.ts` ×1, `useGridTransitionController.ts` ×1, `useViewerMediaPipeline.ts` ×1, `VideoPlayer.tsx` ×1, `Slideshow.tsx` ×1, `useVideoPlayer.ts` ×1, `useFrameTime.ts` ×1, `useBoundaryNavigation.ts` ×1 (intentional, documented).

## Problem
Renderer startup and event lifecycle are spread across too many layers. The app works, but the ownership model is weak: startup wiring, listener registration, theme sync, and runtime task state are assembled from side effects rather than a small number of clearly owned entry points.

## Scope
- `src/app-shell/useAppBootstrap.ts`
- `src/stores/eventBridge.ts`
- `src/stores/taskRuntimeStore.ts`
- Lifecycle-heavy hooks/components with dependency suppression

## Implementation
1. Define which startup concerns belong to:
   - app shell bootstrap
   - event bridge
   - runtime task store
   - view-layer hooks
2. Move listener registration and startup side effects to the smallest stable owner.
3. Remove avoidable `exhaustive-deps` suppressions by making effect ownership explicit.
4. Keep view components from directly owning runtime lifecycle registration where a store/controller should own it.

## Acceptance Criteria
1. App startup responsibilities are clearly split by owner.
2. Event/listener registration does not depend on incidental component composition.
3. Dependency suppression is reduced to cases with a documented reason.
4. Strict Mode/remount behavior is easier to reason about from the code structure alone.

## Test Cases
1. App startup still initializes settings, event bridge, task runtime, and theme exactly once in production.
2. Switching views does not re-register global listeners incorrectly.
3. `npm run test -- --run` and `npx tsc -p tsconfig.json --noEmit` still pass.

## Risk
Medium-high. This is ownership cleanup around startup/runtime wiring; mistakes can create missing listeners or duplicate registration bugs.
