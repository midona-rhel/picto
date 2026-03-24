# PBI-583: Greenfield frontend runtime reconciliation reset

## Priority
P1

## AI-generated caveat
This document is about `src/runtime/**` and the authoritative state-change/reconciliation layer. It is not the controller PBI, not the frontend state-ownership PBI, and not the UI-structure PBI.

## Lifecycle
- `Implemented` when one runtime reconciliation layer exists for the intended slice.
- `Activatable` when `PBI-568` emits the intended state-change information and `PBI-581`/`PBI-582`/`PBI-587` are implemented for the intended slice.
- `Activated` when the intended live flows reconcile through runtime/state-change logic by default.
- `Legacy removed` when replaced broad fallback invalidation paths are deleted for that slice.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)

## Problem
Runtime reconciliation is still too mixed with feature logic and too dependent on fallback refresh behavior.

Current problems:
- targeted refresh logic is still not the only normal correctness path
- resource invalidation and state-change application are harder to follow than they should be
- some domains still rely on broad refresh behavior where authoritative event data should drive the update
- runtime settle behavior is not cleanly landing into one owned frontend state model

## Product model to encode
The runtime layer should own:
- authoritative state-change application
- targeted refresh/resource invalidation
- cross-surface settlement after backend commits

Controllers should do the local visible step. Runtime should do the authoritative settle step.
Frontend state ownership should define where that settled state lives.

## Implementation changes
- centralize state-change application logic in runtime-owned modules
- reduce broad fallback invalidation in the migrated slices
- make state-change contracts explicit per migrated slice

## Acceptance criteria
- migrated slices reconcile primarily through runtime/state-change logic
- broad fallback invalidation is no longer the normal correctness path in those slices
- runtime ownership of authoritative settle behavior is explicit

## Tests
- state-change application tests
- targeted refresh/invalidation tests
- integration tests for optimistic update -> backend commit -> runtime settle

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
