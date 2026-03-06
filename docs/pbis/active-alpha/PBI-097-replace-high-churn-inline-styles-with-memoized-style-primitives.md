# PBI-097: Replace high-churn inline styles with memoized style primitives

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. High-volume inline style usage remains across hot and frequently rerendered surfaces:
   - `/Users/midona/Code/imaginator/src/components/image-grid/ImageGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/CanvasGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/SubscriptionsPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/Collections.tsx`
   - `/Users/midona/Code/imaginator/src/components/VideoPlayer.tsx`
2. Many style objects are recreated every render, reducing memoization effectiveness on child components.

## Problem
Widespread inline style object creation causes avoidable render churn and weakens reusable UI standardization. This also increases visual inconsistency risk.

## Scope
- hot components listed above
- shared style primitives under:
  - `/Users/midona/Code/imaginator/src/styles/`
  - `/Users/midona/Code/imaginator/src/components/ui/`

## Implementation
1. Move repeated inline layout/typography/color style patterns into CSS modules or shared class-based primitives.
2. For dynamic styles that must remain inline, memoize style objects (`useMemo`) and keep value sets minimal.
3. Add lint rule to flag new inline style objects in hot-path components (`image-grid/*`, `sidebar/*`, `settings/*`).
4. Align with token-only color policy (PBI-084) to avoid ad-hoc literals.

## Acceptance Criteria
1. Hot-path components no longer allocate large inline style object trees per render.
2. Component memoization hit rate improves for row/tile child elements.
3. Visual style behavior remains unchanged.

## Test Cases
1. Profile `ImageGrid` and `SubscriptionsPanel` before/after for commit count and render duration.
2. Verify dynamic style behaviors (progress widths, overlays, transitions) still render correctly.
3. Lint check blocks new inline-style regressions in target folders.

## Risk
Low to medium. Mechanical refactor with moderate touch surface.

