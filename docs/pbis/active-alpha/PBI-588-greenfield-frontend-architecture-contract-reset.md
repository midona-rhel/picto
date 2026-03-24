# PBI-588: Greenfield frontend architecture contract reset

## Priority
P1

## AI-generated caveat
This document is the frontend architecture contract. It must be written and treated as binding before more broad frontend rebuild work continues. It is not a styling PBI and not a generic cleanup note.

## Lifecycle
- `Implemented` when the target frontend architecture, ownership rules, and migration checkpoints are written clearly enough to execute without inventing architecture during implementation.
- `Activatable` when `PBI-568`, `PBI-578`, and `PBI-581` are implemented enough to support the first active frontend slice.
- `Activated` when the first migrated frontend slice follows this architecture by default.
- `Legacy removed` when the migrated slices no longer depend on the replaced frontend shapes this contract is forbidding.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)

## Problem
The current frontend reset docs still leave too much architecture to be invented during implementation, and they still assume too much in-place migration inside the current `src/**` tree.

Current problems:
- `App.tsx` is still a cross-domain assembler
- `MainViewRouter.tsx` still pushes a large grid prop surface
- `ImageGrid.tsx` is still the place where too many grid concerns meet
- state is fragmented across legacy Zustand stores, Jotai state, local component state, and reducer-driven runtime state
- controllers still know too much about frontend store internals
- the API layer still mixes canonical and legacy contracts
- the legacy and rebuilt frontends do not yet have a hard boundary

## Fixed architecture rules
Use this frontend shape for the rebuilt active frontend:
- `src/platform/**` is transport only
- `src/controllers/**` is actions only
- `src/state/**` is Jotai-owned domain state
- `src/runtime/**` is backend-confirmed settle/reconciliation
- `src/features/**` contains feature roots and feature composition
- `src/shared/components/**` contains presentational UI and reusable primitives

Use this styling rule:
- the rebuilt frontend styling model is locked by [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)
- rebuilt slices must use tokens, shared primitives, and component-owned CSS Modules instead of copying legacy CSS structure into the new tree

Use this workspace rule:
- the legacy frontend must live outside `src/**`
- the rebuilt active frontend owns `src/**`
- rebuilt product code must not depend on legacy store/controller/runtime modules in its normal execution path

Use this component rule:
- feature roots may read state/controllers/runtime and assemble view-models
- leaf UI components receive small intentional props
- leaf UI components should not know backend contract shapes
- large prop bags across feature boundaries are a design smell and must be reduced
- visually and functionally equivalent UI pieces should be merged into one canonical implementation even if the legacy frontend split them across different features
- rebuilt slices should prefer one reusable primitive plus thin wrappers over several near-identical components with different names

Use this consolidation rule:
- do not preserve legacy component boundaries just because they existed before
- if two surfaces are the same thing with different labels or small behavior flags, rebuild them as one component family
- examples:
  - grid tile preview and inspector preview should share one image/media preview primitive if they look and behave the same
  - sidebar rows should share one row primitive with options for tree nesting, collapse state, drag target state, and selection state
  - repeated rounded image cards, property rows, and panel sections should collapse into one canonical UI family

Use this ownership rule:
- controllers own user actions and optimistic intent only
- runtime owns backend-confirmed settle only
- local `useState` is only for transient UI state
- Jotai owns the intended long-term frontend state

## Required rebuild shape
The frontend reset must run in two parallel tracks.

### Track A: new frontend rebuild
Track A owns:
- shell/sidebar rebuild
- grid screen rebuild
- inspector rebuild
- media/viewer-facing surfaces
- remaining surface rebuilds

### Track B: contract stabilization
Track B owns:
- canonical engine contract stabilization
- frontend API boundary stabilization
- query/selection contract stabilization
- media delivery contract stabilization
- runtime event contract stabilization
- parity/reference harnesses

Track A is not allowed to redesign against moving contracts from Track B.
Track A is also not allowed to “borrow” the legacy frontend architecture into the new active path.
Track A is expected to collapse duplicate legacy UI implementations when the rebuilt UI can preserve the same product behavior with fewer primitives.

## Required checkpoints
### Track A
- `A1`: the legacy frontend has been quarantined out of `src/**`
- `A2`: the rebuilt shell/sidebar slice is live and parity-checked
- `A3`: the rebuilt grid slice is live and parity-checked
- `A4`: the rebuilt inspector/media slice is live and parity-checked
- `A5`: rebuilt slices no longer depend on legacy runtime/store/controller paths

### Track B
- `B1`: canonical entity contract locked
- `B2`: query/selection contract locked
- `B3`: runtime settle contract locked
- `B4`: media contract locked
- `B5`: legacy transport removed for activated slices

### Alignment gates
- `Gate 1`: the shell/sidebar slice may activate only after `A2` and the first parity harness is working
- `Gate 2`: the grid slice may activate only after `A3` and `B2`
- `Gate 3`: the inspector/media slice may activate only after `A4`, `B3`, and `B4`
- `Gate 4`: legacy cleanup for any slice requires `A5` plus that slice’s parity sign-off and stable contract sign-off

## Acceptance criteria
- the frontend architecture is decision-complete before more broad rebuild work continues
- the layer rules are explicit and non-overlapping
- the legacy-vs-new workspace rule is explicit
- the rebuild explicitly allows aggressive consolidation of equivalent UI parts
- the two-track model is explicit
- checkpoints and activation gates are explicit
- later frontend PBIs can reference this document instead of reinventing architecture

## Tests
- architecture review checklist attached to migrated slices
- boundary tests proving slices follow the new layer rules
- slice activation checklist referencing `A*`, `B*`, and gate status

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
