# PBI-557: Consolidate Subscriptions Controller And Window Flows

## AI-Generated Caveat
This PBI is AI-generated and likely to surface more unfinished API design than the initial audit captured. The engineer should expect to find opportunities to simplify the public controller shape substantially while implementing it.

## Priority
P0

## Problem
Subscriptions are one of the most fragmented slices in the app:

- groups
- subscriptions
- queries
- credentials
- schedules
- oauth helper flows
- running/stopping/resetting
- progress reads

These often leak raw backend access directly into UI.

## Goal
Make `subscriptionsController` the single public entrypoint for subscriptions-domain reads and writes.

## Atomicity Rule
This PBI should finish the subscriptions slice. Do not include unrelated settings or general app cleanup unless required for subscription-window helpers.

## Scope

### Controller
- [src/controllers/subscriptionsController.ts](./src/controllers/subscriptionsController.ts)

### Known consumers
- [src/features/subscriptions/components/SubscriptionsWindow.tsx](./src/features/subscriptions/components/SubscriptionsWindow.tsx)
- [src/features/subscriptions/components/SubscriptionGroupsPanel.tsx](./src/features/subscriptions/components/SubscriptionGroupsPanel.tsx)
- [src/features/subscriptions/components/CreateSubscriptionGroupModal.tsx](./src/features/subscriptions/components/CreateSubscriptionGroupModal.tsx)
- [src/features/subscriptions/subscriptionProgressStore.ts](./src/features/subscriptions/subscriptionProgressStore.ts)
- [src/entrypoints/subscriptions.tsx](./src/entrypoints/subscriptions.tsx)

## Required Reads
- groups
- sites
- credentials
- credential health
- running status
- running progress

## Required Writes
- create/rename/delete groups
- run/stop groups
- create/edit/delete subscriptions
- add/edit/delete queries
- reset subscriptions
- set schedules
- set auto-collections
- set/delete credentials
- oauth flows if used in the renderer

## Task Rule
Group run and similar long-running operations must plug into PBI-551’s task orchestration.

## Look For Adjacent Improvements
- simplify oauth helper flow ownership
- collapse overly granular controller methods into clearer ones if that reduces UI duplication
- normalize backend-shaped DTOs in one place instead of in many components
- collapse near-duplicate subscriptions/group/query APIs that represent the same functional operation with minor variation
- remove redundant domain repetition in method names inside the subscriptions controller where the controller already provides that context
- move undo/redo registration for reversible subscriptions actions into the controller when those flows are part of the migrated slice

## Acceptance Criteria
1. All subscriptions-domain reads/writes route through `subscriptionsController`.
2. No raw backend access remains in subscriptions UI and entrypoint code.
3. Long-running subscription actions use centralized task state.
4. Undo/redo is controller-owned for reversible subscriptions actions if PBI-559 is complete.

## Validation
- create/rename/delete group
- create/edit/delete query
- set/reset credentials
- run/stop group
- oauth login flow still works
- undo/redo for any migrated reversible subscriptions actions behaves the same from every surface
