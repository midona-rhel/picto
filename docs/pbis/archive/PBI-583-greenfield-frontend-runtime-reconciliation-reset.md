# PBI-583: Greenfield frontend runtime reconciliation reset

## Priority
P1

## AI-generated caveat
This document is about `src/runtime/**` and the authoritative state-change/reconciliation layer. It is not the controller PBI, not the frontend state-ownership PBI, and not the UI-structure PBI.

## Lifecycle
- `Implemented` when one runtime settle layer exists for the intended slice.
- `Activatable` when `PBI-568` emits the intended state-change information, the runtime event contract is locked, and `PBI-581`/`PBI-582`/`PBI-587` are implemented for the intended slice.
- `Activated` when the intended live flows reconcile through runtime/state-change logic by default.
- `Legacy removed` when replaced broad fallback invalidation paths are deleted for that slice.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)

## Problem
Runtime reconciliation is still too mixed with feature logic and too dependent on fallback refresh behavior.

Current problems:
- targeted refresh logic is still not the only normal correctness path
- resource invalidation and state-change application are harder to follow than they should be
- some domains still rely on broad refresh behavior where authoritative event data should drive the update
- runtime settle behavior is not cleanly landing into one owned frontend state model
- runtime/event semantics are still too easy to expand ad hoc per feature
- the grid still tends to treat “some entity_hash changed” as “reload the grid”
- the sidebar still tends to treat tree/count changes as “fetch the whole sidebar”

## Product model to encode
The runtime layer should own:
- authoritative state-change application
- targeted refresh/resource invalidation
- cross-surface settlement after backend commits

Controllers should do the local visible step. Runtime should do the authoritative settle step.
Frontend state ownership should define where that settled state lives.
Runtime should not become a second state owner and should not carry arbitrary feature-local behavior.
Runtime for rebuilt slices must also stop relying on legacy frontend state/store paths as normal correctness behavior.

For activated rebuilt slices:
- runtime reads current owned frontend query/tree state
- runtime compares current state against backend change facts
- runtime asks the backend to reconcile when membership/order is uncertain
- runtime applies exact sidebar count/node deltas directly when available
- broad sidebar/grid refetch remains fallback only

Runtime event consumption assumes one merged post-commit `runtime/state_changed` per completed backend action.
Do not design runtime settle around event bursts from one logical action.
Runtime may still receive additional later events for distinct async follow-up phases such as compiler batches or deferred derivative completion, and those later events should be handled as separate committed phases.

## Implementation changes
- centralize state-change application logic in runtime-owned modules
- reduce broad fallback invalidation in the migrated slices
- make state-change contracts explicit per migrated slice
- lock which runtime event fields the migrated slice is allowed to depend on
- treat one merged `runtime/state_changed` as the normal event unit for one completed action
- teach runtime/grid settle to use current query plus scope-impact facts instead of hash-only invalidation
- teach runtime/sidebar settle to apply count/tree deltas instead of broad tree fetch as the normal path

## Acceptance criteria
- migrated slices reconcile primarily through runtime/state-change logic
- broad fallback invalidation is no longer the normal correctness path in those slices
- runtime ownership of authoritative settle behavior is explicit
- runtime is settle-only for activated slices
- runtime logic assumes one merged state-change event per completed action, with extra later events only for distinct async follow-up phases
- grid settlement is query-aware and backend-reconciled for membership/order changes
- sidebar settlement is delta-driven by default and broad tree fetch is an explicit fallback only

## Tests
- state-change application tests
- targeted refresh/invalidation tests
- integration tests for optimistic update -> backend commit -> runtime settle
- query reconciliation decision tests for current grid state vs event scope impact
- sidebar delta application tests

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
