# PBI-082: Reduce App->Router prop drilling with view-model context

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. `MainViewRouter` defines a very large prop contract:
   - `/Users/midona/Code/imaginator/src/components/layout/MainViewRouter.tsx:14-48`
2. `App` passes the full prop surface directly:
   - `/Users/midona/Code/imaginator/src/App.tsx:419-453`

## Problem
High-volume prop drilling from app shell to main view router increases coupling and makes reuse/testing difficult. Small feature changes require touching multiple unrelated callsites.

## Scope
- `/Users/midona/Code/imaginator/src/App.tsx`
- `/Users/midona/Code/imaginator/src/components/layout/MainViewRouter.tsx`
- new view-model context/hooks module under `/src/components/layout/` or `/src/stores/`

## Implementation
1. Introduce a `MainViewModel` provider/hook with grouped state slices:
   - navigation scope
   - grid query/view config
   - selection + detail state
   - flows state
2. Replace wide prop passing with stable typed selectors/hooks.
3. Keep explicit prop boundaries only where true component API is needed (e.g. callbacks crossing feature boundaries).
4. Add lightweight tests for route rendering based on view-model state.

## Acceptance Criteria
1. `MainViewRouter` prop surface is significantly reduced (target: minimal routing props).
2. `App` no longer forwards dozens of grid/selection/flow props directly.
3. Behavior remains unchanged across images/flows/tags/duplicates routes.

## Test Cases
1. Route switch tests for each main view using provider-backed state.
2. Scope/filter changes still update grid view correctly.
3. Detail/selection callbacks continue to work after refactor.

## Risk
Medium. State wiring refactor across core shell routing path.

