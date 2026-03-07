# PBI-244: Controller-driven view transition lifecycle

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. View transitions (grid → detail, detail → grid, grid scope A → grid scope B) are driven by view components, not controllers.
2. `ImageGrid` dispatches `OPEN_DETAIL` / `CLOSE_DETAIL` on double-click. `DetailView` calls `onClose`. The views own the transitions.
3. Detail state is split across three locations:
   - `detailHash` + `viewerSession` in `GridRuntimeState` (gridRuntimeState.ts)
   - `detailViewState` + `detailViewControls` in `useInspectorState` (App.tsx)
   - `isDetailMode` derived from `!!detailViewState`
4. `useGridTransitionController` mixes scope transitions (folder changes), geometry changes (view mode, target size), and detail state clearing — three concerns in one hook.
5. Scope changes implicitly close the detail view as a side effect inside `useGridTransitionController` (dispatches `CLOSE_DETAIL` + `onDetailViewStateChange(null, null)`) with no clear abstraction.
6. `onDetailViewStateChange` is prop-drilled through 4+ layers: App → MainViewRouter → ImageGrid → useGridScopeTransition → DetailView.

## Problem
View transitions are scattered across components, hooks, and reducers with no single owner. There is no function you can call that says "transition from grid to detail for this hash" or "transition from detail back to grid" and have it handle the full lifecycle. Instead, transitions are assembled from side effects spread across:
- Grid item click handlers
- Grid runtime reducer (`OPEN_DETAIL`, `CLOSE_DETAIL`)
- `useGridTransitionController` (scope transitions, implicit detail close)
- `useInspectorState` (detail view state for titlebar)
- App-level prop drilling

This makes the transition flow very hard to follow and fragile to change. Adding a new transition behavior (e.g. animation, preloading) requires touching multiple files.

## Desired model

A **view transition controller** that owns the full lifecycle of every view change:

```
ViewTransitionController
  ├── openDetail(hash)        — grid → detail
  ├── closeDetail()           — detail → grid
  ├── navigateDetail(hash)    — detail → detail (next/prev)
  ├── changeScope(scope)      — grid scope A → grid scope B (with fade)
  └── state                   — current view, transition phase, active hash
```

Each function is the single entry point for its transition. The controller:
1. Manages the full state machine (idle → transitioning → settled)
2. Coordinates detail state, grid state, and inspector state in one place
3. Replaces the prop drilling — components read from the controller, not from 3 separate stores
4. Handles edge cases (e.g. scope change while detail is open → close detail first, then transition)

## Scope
- `src/components/image-grid/gridRuntimeState.ts` — extract detail/transition state
- `src/components/image-grid/ImageGrid.tsx` — remove transition ownership, call controller
- `src/components/image-grid/useGridTransitionController.ts` — refactor into the new controller
- `src/components/image-grid/useGridScopeTransition.ts` — fold into controller
- `src/App.tsx` — remove `useInspectorState` detail state management, read from controller
- New: `src/controllers/ViewTransitionController.ts` (or `src/stores/viewTransitionStore.ts`)

## Implementation
1. Create a `ViewTransitionController` (Zustand store or context) that holds:
   - `currentView`: `'grid' | 'detail'`
   - `transitionPhase`: `'idle' | 'fading_out' | 'loading' | 'fading_in'`
   - `detailHash`: current detail hash (or null)
   - `viewerSession`: current session (images list, index)
   - `activeScope`: current grid scope
2. Implement transition functions (`openDetail`, `closeDetail`, `changeScope`, etc.) as actions on the store/controller.
3. Each transition function manages the full sequence:
   - `openDetail(hash)`: set detailHash → create viewerSession → set currentView to detail → notify inspector
   - `closeDetail()`: clear detailHash → clear viewerSession → set currentView to grid → notify inspector
   - `changeScope(scope)`: if detail open → closeDetail first → start fade → load new scope → commit → fade in
4. Remove `OPEN_DETAIL` / `CLOSE_DETAIL` from grid runtime reducer — these actions now go through the controller.
5. Remove `onDetailViewStateChange` prop drilling — components read `detailViewState` from the controller directly.
6. Grid item click handlers call `controller.openDetail(hash)` instead of `dispatch({ type: 'OPEN_DETAIL' })`.
7. `DetailView` calls `controller.closeDetail()` instead of `onClose` prop.

## Relationship to other PBIs
- **PBI-098** (merge DetailView/DetailWindow/QuickLook): this PBI handles the transition *to* the viewer, PBI-098 handles the viewer itself. They are complementary — PBI-244 defines how you get into and out of the viewer, PBI-098 defines what the viewer does once you're in it.
- **PBI-087** (centralize frontend domain mutations): the view transition controller is a domain mutation controller — this PBI is a concrete instance of the pattern PBI-087 describes.

## Acceptance Criteria
1. A single controller/store owns all view transition state.
2. `openDetail(hash)` is the only way to enter detail view — no direct reducer dispatches.
3. `closeDetail()` is the only way to exit detail view.
4. `changeScope(scope)` handles scope transitions including implicit detail close.
5. No `onDetailViewStateChange` prop drilling through 4+ layers.
6. Transition state is readable from one location, not derived from 3 separate stores.
7. All existing transitions work identically from the user's perspective.

## Test Cases
1. Double-click grid image → detail opens (via controller).
2. Press Escape in detail → grid returns (via controller).
3. Click sidebar folder while in detail → detail closes, grid transitions to new scope.
4. Navigate next/prev in detail → session updates via controller.
5. Rapid transitions (open detail, immediately change scope) — no stale state or race conditions.
6. Open in new window — still works (separate path, unaffected).

## Risk
Medium-high. Touches the core navigation flow. Must be carefully staged — extract the controller first with the existing behavior, then simplify the components to use it. Do not change behavior and ownership in the same step.

## Audit Addendum (2026-03-07)

Concrete frontend audit findings that belong under this PBI:

1. `src/App.tsx` still coordinates shell composition, command palette wiring, scoped grid preferences, inspector state, and transition midpoint handling in one place.
2. `src/app-shell/useAppBootstrap.ts` currently owns startup wiring, event bridge setup, task runtime initialization, theme sync, and titlebar drag behavior, which makes transition/runtime ownership harder to follow.
3. Multiple lifecycle-heavy modules still rely on `react-hooks/exhaustive-deps` suppression, which is a signal that ownership boundaries are still too blurry around view transitions and bootstrap.
