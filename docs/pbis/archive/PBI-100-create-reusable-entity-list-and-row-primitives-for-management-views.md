# PBI-100: Create reusable entity-list and row primitives for management views

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Management surfaces implement custom row/action layouts repeatedly:
   - `/Users/midona/Code/imaginator/src/components/settings/SubscriptionsPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/FlowsWorking.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/LibraryPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/Collections.tsx`
2. Same patterns recur:
   - left label + metadata
   - right compact action buttons
   - expandable secondary rows
   - empty/loading blocks

## Problem
Repeated list-row implementations increase maintenance cost and UI inconsistency, while making performance tuning (memoization/virtualization) harder to apply consistently.

## Scope
- views listed above
- new primitives in `/Users/midona/Code/imaginator/src/components/ui/list/`

## Implementation
1. Introduce shared primitives:
   - `EntityList`
   - `EntityRow`
   - `RowActions`
   - optional `ExpandableRowBody`
2. Migrate subscriptions/flows/library first (highest overlap).
3. Centralize row keyboard focus/selection and action slot rendering.
4. Add memoized row rendering and stable key policies by default.

## Acceptance Criteria
1. Core management screens use shared list-row primitives.
2. Row spacing/typography/actions are consistent.
3. Performance tuning is applied once and reused (memoized rows, optional virtualization).

## Test Cases
1. Subscription row expand/collapse + actions parity.
2. Flow row actions and progress display parity.
3. Library history row actions and rename flow parity.

## Risk
Medium. UI composition refactor with significant reuse benefits.

