# PBI-570: Greenfield frontend reset program index

## Priority
P1

## AI-generated caveat
This document is intentionally a program index, not an implementation slice. The frontend is too large and too entangled to treat as one executable PBI. This file exists to define the frontend target architecture, the child PBIs, and the activation order.

## Lifecycle
- `Implemented` when the child frontend reset PBIs are written clearly enough to execute in slices.
- `Activatable` when the dependency order is explicit and the backend reset line is implemented enough to support the frontend work.
- `Activated` when the intended frontend slices are using the new architecture by default.
- `Legacy removed` when the replaced frontend paths are deleted from the activated slices.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-569-greenfield-media-delivery-service.md](./docs/pbis/active-alpha/PBI-569-greenfield-media-delivery-service.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)

## Problem
The frontend needs a real re-engineer, not one vague cleanup PBI.

Current problems:
- the API layer is still too close to raw transport and legacy command shapes
- controller ownership is inconsistent across domains
- frontend state ownership and view-model shape are still too loose
- runtime reconciliation is still mixed with feature logic and fallback refresh behavior
- the grid path is still split between old query shapes and new canonical behavior
- media consumption still carries path-shaped assumptions
- frontend feature/module boundaries are too loose and duplicated
- component and styling duplication are separate problems, but they are still being treated as if they were the whole frontend reset

## Frontend target architecture
The frontend should end up with these layers:
- `src/platform/**` as the frontend API layer and transport adapter
- `src/controllers/**` as domain behavior boundaries
- explicit state-ownership/view-model layers for migrated slices
- `src/runtime/**` as authoritative backend reconciliation and refresh targeting
- `src/features/**` as domain composition, not backend/transport plumbing
- `src/shared/components/**` and `src/shared/styles/**` as the reusable UI system

Feature code should not directly own:
- backend command names
- raw transport calls
- ad hoc selection semantics
- duplicated domain-state storage or competing view-model builders
- path-based media assumptions
- repeated shell/component implementations that should be shared

## Child PBIs
This program is executed through these child PBIs:

1. [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
2. [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
3. [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
4. [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
5. [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
6. [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md)
7. [PBI-586-greenfield-frontend-feature-module-architecture-reset.md](./docs/pbis/active-alpha/PBI-586-greenfield-frontend-feature-module-architecture-reset.md)
8. [PBI-571-frontend-shared-component-and-styling-system-reset.md](./docs/pbis/active-alpha/PBI-571-frontend-shared-component-and-styling-system-reset.md)

## Activation order
Use this order for the frontend program:

1. Activate the frontend API layer:
- `PBI-581`

2. Activate controller ownership:
- `PBI-582`

3. Activate frontend state ownership:
- `PBI-587`

4. Activate runtime/state-change reconciliation:
- `PBI-583`

5. Activate the core grid/query/selection flow:
- `PBI-584`

6. Activate media consumption on the media delivery service:
- `PBI-585`

7. Activate the frontend feature/module architecture:
- `PBI-586`

8. Activate shared UI/component/styling consolidation:
- `PBI-571`

Some implementation work can overlap, but activation should generally follow this order.

## Relationship to other reset PBIs
- `PBI-567` defines the library storage boundary
- `PBI-568` defines the backend engine boundary
- `PBI-569` defines media delivery
- `PBI-578` defines bulk target and selection semantics
- `PBI-570` defines the frontend program and child-PBI order
- `PBI-571` and `PBI-581` through `PBI-587` execute the frontend reset in slices

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This program index is complete only when:
- the frontend reset is no longer treated as one vague implementation blob
- each major frontend architecture problem has its own executable child PBI
- the activation order is explicit
- dependencies on backend and media delivery are explicit
