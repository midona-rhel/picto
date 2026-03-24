# PBI-584: Greenfield frontend grid query and selection reset

## Priority
P1

## AI-generated caveat
This document is about the frontend grid/query/selection path specifically. It exists because that slice is too important and too subtle to bury inside a generic frontend-boundary PBI.

## Lifecycle
- `Implemented` when the frontend grid/query/selection path has one clear canonical model in code.
- `Activatable` when `PBI-568`, `PBI-578`, `PBI-581`, `PBI-583`, and `PBI-587` are implemented enough for the real grid/query/selection flow.
- `Activated` when the live grid and selection-driven entity actions use the canonical query/target model by default.
- `Legacy removed` when replaced slim/grid-page selection paths for that slice are deleted.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)

## Problem
The grid and selection path is still split between old view models and new canonical backend semantics.

Current problems:
- `get_grid_page_slim` still anchors important frontend behavior
- selection semantics still carry old `SelectionQuerySpec` behavior that does not line up cleanly with canonical backend targets
- bulk actions are still easy to get wrong when they originate from filtered or virtual grid selections
- grid-visible state and selection state are still not clearly owned enough

## Product model to encode
The frontend grid path should:
- build one canonical typed entity-view query
- drive selection actions through the same canonical target semantics
- keep top-level grouped grid semantics aligned with what backend bulk actions will target

## Implementation changes
- migrate frontend grid data flow away from slim/grid-page legacy contract
- align virtual selection and select-all behavior with canonical backend targets
- remove duplicated selection translation logic where possible
- make grouped top-level selection semantics explicit and testable

## Acceptance criteria
- the main grid uses the canonical entity-view query model
- selection-driven entity actions target what the user actually selected
- virtual selection/select-all semantics are stable and explicit
- slim/grid-page legacy dependence is materially reduced or removed for the activated slice

## Tests
- grid query construction tests
- selection-target translation tests
- grouped selection behavior tests
- integration tests for filtered selection -> action -> runtime settle

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
