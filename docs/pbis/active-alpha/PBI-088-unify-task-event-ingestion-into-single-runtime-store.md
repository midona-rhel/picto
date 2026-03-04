# PBI-088: Unify task/event ingestion into single runtime store

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Multiple surfaces independently subscribe to overlapping PTR/subscription events:
   - `/Users/midona/Code/imaginator/src/components/layout/SidebarJobStatus.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/PtrPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/settings/SubscriptionsPanel.tsx`
   - `/Users/midona/Code/imaginator/src/components/FlowsWorking.tsx`
2. Polling fallbacks are duplicated and can diverge from event behavior:
   - `/Users/midona/Code/imaginator/src/components/layout/SidebarJobStatus.tsx` (watchdog poll)
   - `/Users/midona/Code/imaginator/src/components/settings/PtrPanel.tsx` (1.5s poll loop)

## Problem
Task lifecycle state is derived separately in each component. This duplicates listener setup/teardown logic, risks inconsistent status rendering, and increases event load and stale-state bugs.

## Scope
- `/Users/midona/Code/imaginator/src/components/layout/SidebarJobStatus.tsx`
- `/Users/midona/Code/imaginator/src/components/settings/PtrPanel.tsx`
- `/Users/midona/Code/imaginator/src/components/settings/SubscriptionsPanel.tsx`
- `/Users/midona/Code/imaginator/src/components/FlowsWorking.tsx`
- new store: `/Users/midona/Code/imaginator/src/stores/taskRuntimeStore.ts`

## Implementation
1. Create `taskRuntimeStore` as the single owner of:
   - PTR sync/bootstrap state
   - subscription running/progress/finished state
   - flow lifecycle status
2. Register all runtime listeners once in the store bootstrap.
3. Move watchdog polling into the store only (single fallback loop with stale-event gating).
4. Convert UI components to read selectors from the store instead of wiring direct listeners.

## Acceptance Criteria
1. Event listener ownership is centralized to one runtime store.
2. Sidebar and settings/task views render the same canonical task state.
3. Polling is singular and bounded; no per-component duplicate polls.

## Test Cases
1. PTR sync start/progress/finish updates both sidebar and settings without separate listeners.
2. Subscription/flow events update all task UIs consistently.
3. Simulated event loss recovers via single watchdog poll without duplicate state churn.

## Risk
Medium. Cross-cuts task UX surfaces but eliminates repeated event wiring and drift.

