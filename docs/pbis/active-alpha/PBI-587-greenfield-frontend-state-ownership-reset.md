# PBI-587: Greenfield frontend state ownership reset

## Priority
P1

## AI-generated caveat
This document is about frontend state ownership and view-model shape. It is not the API-layer PBI, not the controller PBI, and not the runtime reconciliation PBI.

## Lifecycle
- `Implemented` when migrated frontend slices have one clear state-ownership model in code.
- `Activatable` when `PBI-581` and `PBI-582` are implemented enough for the intended slice.
- `Activated` when the intended frontend flows use the new state-owned path by default.
- `Legacy removed` when replaced ad hoc local state, duplicated selectors, and overlapping view-model paths are deleted for that slice.

Activation depends on:
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)

## Problem
Frontend state is still too loose, too duplicated, and too hard to reason about.

Current problems:
- the same visible state is recomputed or stored in several nearby places
- hooks, controllers, and feature components still compete for state ownership
- view-model shape is not consistent enough across grid, inspector, sidebar, and dialogs
- local optimistic state and authoritative settled state are not clearly separated
- selectors and derived state logic are duplicated across features

## Product model to encode
The frontend should have one clear answer for each migrated slice:
- what state is authoritative frontend state
- what state is transient UI state
- what state is derived and should not be stored twice
- what layer owns each piece of state
- how optimistic local state becomes authoritative settled state

Controllers should trigger domain actions.
Runtime should settle backend-confirmed state.
State ownership should define what the frontend stores between those two steps.

## Required shape
- one explicit state owner per migrated slice
- view-model hooks read from owned state instead of rebuilding equivalent state ad hoc
- derived state is centralized and reusable
- transient UI state is separated from domain state
- duplicated stores, duplicated selectors, and duplicated visible-state builders are reduced aggressively

## Implementation changes
- define one state-ownership model for the migrated slices
- centralize derived selectors/view-model builders where the same visible state is used across surfaces
- remove duplicate state caches that only exist because ownership is unclear
- make the boundary between domain state, derived state, and transient UI state explicit
- keep state logic out of random feature components

## Acceptance criteria
- migrated slices have one explainable state owner
- the same visible state is no longer rebuilt or stored in several competing places
- domain state, derived state, and transient UI state are clearly separated
- view-model hooks for migrated slices become thinner because ownership is clearer

## Tests
- state-ownership tests for migrated slices
- selector/derived-state tests where reuse matters
- integration tests for optimistic local step -> authoritative settle -> final visible state
- boundary tests proving feature components are not owning duplicated domain state logic

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
