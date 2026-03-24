# PBI-592: Greenfield frontend grid screen rebuild

## Priority
P1

## AI-generated caveat
This document is about rebuilding the grid screen as a clean surface in the new frontend. It is not an in-place refactor of `ImageGrid.tsx`.

## Lifecycle
- `Implemented` when the rebuilt grid screen exists as a clean feature root in the new `src/**` tree.
- `Activatable` when canonical query/selection contracts are locked and parity fixtures exist.
- `Activated` when the rebuilt grid screen is the live path.
- `Legacy removed` when the legacy grid-screen path is deleted.

Activation depends on:
- [PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md](./docs/pbis/active-alpha/PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md)
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)

## Problem
The current grid screen is spread across:
- `App.tsx`
- `MainViewRouter.tsx`
- `ImageGrid.tsx`
- many grid hooks
- legacy stores
- runtime reducers

That is too entangled to keep patching.

## Product model to encode
The rebuilt grid screen should have:
- one grid feature root
- one canonical query model
- one selection model
- one results/pagination model
- one renderer boundary
- one viewer bridge

The rebuilt grid screen should not depend on:
- the legacy `ImageGrid.tsx` architecture
- legacy slim-grid contracts
- giant prop bags from the app shell

## Acceptance criteria
- the rebuilt grid screen is a clean feature root
- canonical query/selection semantics are used by default
- selection-driven actions target what the user actually selected
- parity is confirmed against the reference harness
- the rebuilt grid no longer depends on the legacy `ImageGrid.tsx` architecture

## Tests
- fixture rendering tests for grid states
- query construction tests
- selection-target translation tests
- parity checklist and visual confirmation notes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
