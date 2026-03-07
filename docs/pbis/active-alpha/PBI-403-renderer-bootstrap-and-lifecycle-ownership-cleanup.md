# PBI-403: Renderer bootstrap and lifecycle ownership cleanup

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `src/app-shell/useAppBootstrap.ts` owns window startup, theme sync, event bridge setup, task runtime initialization, menu listeners, and titlebar drag behavior.
2. The frontend still carries multiple `react-hooks/exhaustive-deps` suppressions in lifecycle-heavy modules.
3. Runtime/event ownership is split between `useAppBootstrap`, `setupEventBridge`, `useTaskRuntimeStore`, and view components.

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
