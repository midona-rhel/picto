# PBI-518: Shared ZoomableImage component extraction

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The duplication finding is based on observing similar zoom/pan/navigator patterns in two components. The actual degree of code overlap and the feasibility of unification should be verified by reading both implementations side by side. It's possible the two use cases are different enough that sharing would be forced abstraction.

## Priority
P3

## Problem
`DuplicateManager.tsx` (663 LOC) and `MediaView.tsx` (719 LOC) both implement zoom, pan, and navigator overlay logic independently:

- Both use refs for zoom containers, image elements, and navigator state
- Both implement mouse-drag panning with pointer events
- Both render a navigator overlay (minimap) for panning large images
- Both calculate zoom levels, fit-to-container logic, and scroll-to-center behavior

This is a maintenance risk: bug fixes or UX improvements to zoom/pan in one component may not be propagated to the other.

## Scope
- Compare zoom/pan logic in both components to quantify actual overlap
- If significant: extract a shared `ZoomableImage` component or hook
- If not: document the differences and close this PBI

## Implementation
1. Read `src/features/duplicates/components/DuplicateManager.tsx` zoom logic in detail.
2. Read `src/features/viewer/components/MediaView.tsx` zoom logic in detail.
3. Identify shared patterns: zoom level calculation, pan handlers, navigator rendering, fit-to-container.
4. If overlap > 60%: Extract `useZoomPan` hook or `ZoomableImage` component into `src/shared/components/`.
5. Refactor both consumers to use the shared primitive.
6. If overlap < 60%: Document why they diverge and close this PBI.

## Acceptance Criteria
1. Zoom/pan behavior in both MediaView and DuplicateManager is identical to before.
2. No regression in zoom/pan UX in either context.
3. If extracted: shared component/hook exists in `src/shared/` with a single source of zoom logic.

## Test Cases
1. MediaView: Open detail → zoom in with scroll wheel → pan by dragging → navigator shows correct viewport.
2. DuplicateManager: Open duplicate pair → zoom in → pan → navigator works.
3. Both contexts: Fit-to-container on window resize.

## Risk
Low. This is a refactoring task with no behavioral change. The risk is creating a forced abstraction that doesn't fit both use cases well — the "compare first" step mitigates this.
