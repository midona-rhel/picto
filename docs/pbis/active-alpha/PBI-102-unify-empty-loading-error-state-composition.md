# PBI-102: Unify empty/loading/error state composition

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. `EmptyState` exists but many surfaces still hand-roll fallback blocks:
   - `/Users/midona/Code/imaginator/src/components/image-grid/ImageGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/CanvasGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/LibraryPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/SubscriptionsPanel.tsx`
2. Loading and error fragments are implemented ad-hoc in large components (often with inline styles).

## Problem
State handling UI (empty/loading/error/retry) is inconsistent and duplicated, making behavior fixes and styling changes expensive and error-prone.

## Scope
- files listed above
- `/Users/midona/Code/imaginator/src/components/ui/EmptyState.tsx`
- new shared state component(s) under `/Users/midona/Code/imaginator/src/components/ui/state/`

## Implementation
1. Introduce shared state composition primitives:
   - `StateBlock` (loading/empty/error variants)
   - `StateActions` (retry/import/create action row)
2. Replace ad-hoc fallback sections in high-traffic views (grid + settings first).
3. Ensure consistent spacing, typography, and action affordances.
4. Keep i18n/message text at call site, structure in primitive.

## Acceptance Criteria
1. Empty/loading/error UI patterns are consistent across major surfaces.
2. Fallback rendering code volume in large components is reduced.
3. Retry/import/create action wiring remains intact.

## Test Cases
1. Grid empty + error + loading states.
2. Subscriptions/library empty states.
3. Duplicate manager and collections empty state parity.

## Risk
Low. Mostly UI composition cleanup with strong reuse benefits.

