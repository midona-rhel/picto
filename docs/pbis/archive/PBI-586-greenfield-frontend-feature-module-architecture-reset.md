# PBI-586: Greenfield frontend feature and module architecture reset

## Priority
P1

## AI-generated caveat
This is the actual frontend re-engineer PBI. It is intentionally separate from the API layer, controller, runtime, grid, and media consumption PBIs because the module-structure problem is larger than any one of those slices.

## Lifecycle
- `Implemented` when the frontend has a clear target module structure in code for the intended slices.
- `Activatable` when `PBI-581`, `PBI-582`, `PBI-587`, `PBI-583`, `PBI-584`, and `PBI-585` are implemented enough that feature/module cleanup is not fighting unresolved boundary churn.
- `Activated` when migrated frontend domains use the new module architecture by default.
- `Legacy removed` when replaced loose/duplicated module paths for those slices are deleted.

Activation depends on:
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
- [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md)

## Problem
The frontend module structure is still too loose, too duplicated, and too unclear in purpose.

Current problems:
- feature/module ownership is not obvious enough
- multiple nearby folders carry overlapping responsibilities
- logic and composition layers are mixed too freely
- shared helpers and shared components are not cleanly distinguished from feature-specific ones

## Product model to encode
The frontend module architecture should make these boundaries obvious:
- API layer
- controllers
- state ownership and view-models
- runtime reconciliation
- feature composition
- shared UI system
- shared utilities/types

It should be easy to answer:
- where backend calls live
- where domain behavior lives
- where migrated frontend state is owned
- where authoritative state reconciliation lives
- where shared UI primitives live
- where feature-specific behavior lives

It should also make this explicit:
- modules are organized by actual ownership, not by the accidental legacy screen where a UI piece first appeared
- equivalent UI parts should move into shared primitive families instead of staying trapped under old feature folders

## Implementation changes
- define and land a target module layout for the migrated slices
- move files so ownership becomes clearer
- remove overlapping helper modules where one owned home is enough
- stop letting feature folders duplicate shared concerns
- move equivalent UI parts into shared homes even when that breaks legacy feature-folder boundaries

## Start gate
This PBI may start only when:
- the rebuilt shell/sidebar slice from [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md) is `Activated`
- the rebuilt grid slice from [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md) is `Activated`
- the rebuilt inspector slice from [PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md](./docs/pbis/active-alpha/PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md) is `Activated`
- the rebuilt media slice from [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md) is `Activated`

## Next rule
Do not start [PBI-571-frontend-shared-component-and-styling-system-reset.md](./docs/pbis/active-alpha/PBI-571-frontend-shared-component-and-styling-system-reset.md) until this PBI is `Activated`.

## Acceptance criteria
- migrated slices have a clear and explainable module structure
- ownership boundaries are materially clearer than before
- duplicated or overlapping module responsibilities are reduced
- legacy feature boundaries are not preserved when they block obvious UI consolidation
- new contributors can tell where a change belongs without guessing across several folders

## Tests
- boundary tests where helpful
- focused regression tests for moved slices
- module ownership review checklist in the PBI notes or implementation PR

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
