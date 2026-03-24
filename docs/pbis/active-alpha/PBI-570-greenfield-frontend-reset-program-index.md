# PBI-570: Greenfield frontend reset program index

## Priority
P1

## AI-generated caveat
This document is intentionally a program index, not an implementation slice. The frontend is too large and too entangled to fix by continuing in-place migration inside the current `src/**` tree. This file defines the rebuild strategy, the child PBIs, and the activation order.

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
The frontend needs a true rebuild, not another long-lived in-place migration.

Current code facts:
- `src/**` is still about 50k lines
- `App.tsx` is still a 474-line cross-domain root
- `ImageGrid.tsx` is still an 889-line architecture choke point
- `InspectorPanel.tsx` is still about 800 lines
- `FilterBar.tsx` is still about 454 lines
- `src/platform/api.ts` is still about 670 lines
- legacy stores, new Jotai slices, controllers, runtime reducers, and large feature components are all interleaved in the same active tree

That means the current “migrate slice by slice inside `src/**`” approach keeps producing churn:
- old and new architecture stay alive together
- the active route breaks easily during migration
- AI-assisted implementation keeps trying to patch the old architecture instead of replacing it
- there is no clean place to build the new frontend without dragging old assumptions along

## Frontend rebuild strategy
The reset now follows one hard decision:
- move the current frontend implementation out of `src/**`
- treat that moved code as `legacy frontend`
- rebuild the active frontend in a clean `src/**` tree from scratch

The legacy frontend exists for:
- behavior reference
- visual reference
- targeted parity checks
- copying product intent, not architecture

The new frontend exists for:
- the actual product path
- new state ownership
- new component boundaries
- new backend contract usage
- aggressively reduced UI duplication
- a clean styling model based on tokens, shared primitives, and component-owned CSS Modules

## Frontend target architecture
The rebuilt frontend should end up with these layers:
- `src/platform/**` as the frontend API layer and transport adapter
- `src/controllers/**` as domain action boundaries
- `src/state/**` as the owned frontend state layer
- `src/runtime/**` as authoritative backend reconciliation and refresh targeting
- `src/features/**` as feature roots and composition only
- `src/shared/components/**` and `src/shared/styles/**` as the reusable UI system

The moved legacy frontend should live outside `src/**`, under a path such as:
- `legacy/frontend/**`

The new frontend must not depend on legacy runtime/store/controller code as part of normal product execution.

This architecture is locked by:
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)
- [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)

Feature code should not directly own:
- backend command names
- raw transport calls
- ad hoc selection semantics
- duplicated domain-state storage or competing view-model builders
- path-based media assumptions
- repeated shell/component implementations that should be shared

Equivalent rebuilt UI parts should be merged aggressively.
The frontend program should not preserve separate legacy components when they are really one UI family with minor variants.
Examples:
- grid preview tile and inspector preview can become one preview component family
- sidebar rows can become one row family with tree/collapse/drag variants
- repeated rounded image cards, panel headers, and property rows should collapse into shared primitives

## Child PBIs
This program is executed through these child PBIs:

1. [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)
2. [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)
3. [PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md](./docs/pbis/active-alpha/PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md)
4. [PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md](./docs/pbis/active-alpha/PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md)
5. [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
6. [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
7. [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
8. [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
9. [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md)
10. [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
11. [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md)
12. [PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md](./docs/pbis/active-alpha/PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md)
13. [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md)
14. [PBI-586-greenfield-frontend-feature-module-architecture-reset.md](./docs/pbis/active-alpha/PBI-586-greenfield-frontend-feature-module-architecture-reset.md)
15. [PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md](./docs/pbis/active-alpha/PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md)
16. [PBI-596-greenfield-random-active-image-view-contract.md](./docs/pbis/active-alpha/PBI-596-greenfield-random-active-image-view-contract.md)
17. [PBI-571-frontend-shared-component-and-styling-system-reset.md](./docs/pbis/active-alpha/PBI-571-frontend-shared-component-and-styling-system-reset.md)

## Two tracks
Run the frontend reset in two parallel tracks:

### Track A: new frontend rebuild
Track A owns:
- creating the clean `src/**` tree
- rebuilding app shell and sidebar
- rebuilding the grid screen
- rebuilding inspector and viewer-adjacent surfaces
- rebuilding remaining non-image surfaces

Track A is a rewrite track.
It should not spend time repairing the legacy architecture except where needed to keep a parity reference alive.

### Track B: contract and verification track
Track B owns:
- canonical backend/frontend contract stabilization
- frontend API boundary stabilization
- controller/state/runtime rules for the rebuilt slices
- query and selection semantics
- media delivery contract stabilization
- visual reference fixtures and parity harnesses

Track B exists so Track A can rebuild against stable rules and stable checkpoints instead of inventing architecture while coding.

## Execution rules
Track A owns rebuilt live product slices and must run serially.

Track A sequence:
- [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md)
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md)
- [PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md](./docs/pbis/active-alpha/PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md)
- [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md)
- [PBI-586-greenfield-frontend-feature-module-architecture-reset.md](./docs/pbis/active-alpha/PBI-586-greenfield-frontend-feature-module-architecture-reset.md)
- [PBI-571-frontend-shared-component-and-styling-system-reset.md](./docs/pbis/active-alpha/PBI-571-frontend-shared-component-and-styling-system-reset.md)

Rules:
- do not start the next Track A PBI until the current Track A PBI is `Activated`
- Track A “done enough to move on” means `Activated`, not merely `Implemented`
- temporary TODOs are allowed only for cross-PBI boundaries already named in the dependency list; they do not allow the next Track A PBI to start early

Track B owns contract, state, runtime, and verification stabilization and may overlap.

Track B sequence:
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)

Rules:
- Track B PBIs may run in parallel
- a Track A PBI may start only when all of its Track B dependencies are at least `Implemented` and review-clean
- Track B overlap exists to satisfy the next Track A start gate, not to justify starting rebuilt live slices early

## Activation order
Use this order for the frontend program:

1. Lock the rebuild contract:
- `PBI-588`
- `PBI-594`

2. Quarantine the old frontend out of `src/**`:
- `PBI-589`

3. Build the parity/reference harness:
- `PBI-590`

4. Stabilize the new frontend contract surface:
- `PBI-581`
- `PBI-582`
- `PBI-587`
- `PBI-583`
- `PBI-584`

5. Rebuild the first live product slice:
- `PBI-591` app shell and sidebar

6. Rebuild the main image flow:
- `PBI-592` grid screen
- `PBI-593` inspector and selection surface
- `PBI-585` media consumption

7. Rebuild the remaining frontend module structure:
- `PBI-586`

8. Rebuild manager navigation and manager-style tool surfaces:
- `PBI-595`

9. Add the random active-image view once the rebuilt grid/query path is stable:
- `PBI-596`

10. Consolidate shared UI and styling only after the rebuilt surfaces are stable:
- `PBI-571`

Contract and verification work may overlap as part of Track B.
Rebuilt live product slices in Track A should follow this order strictly.

## Alignment gates
- `Gate 1`: the new shell/sidebar slice may activate only when `PBI-591` has parity confirmation against `PBI-590` fixtures and does not depend on legacy runtime/store modules
- `Gate 2`: the new grid slice may activate only when `PBI-584` query/selection semantics are locked and `PBI-592` has parity confirmation against `PBI-590` fixtures
- `Gate 3`: the new inspector/media slice may activate only when `PBI-593` and `PBI-585` have parity confirmation and the media contract from `PBI-569` is stable
- `Gate 4`: legacy cleanup for a slice may happen only when the rebuilt slice is live, parity-checked, and no longer depends on legacy transport or state paths

## Relationship to other reset PBIs
- `PBI-567` defines the library storage boundary
- `PBI-568` defines the backend engine boundary
- `PBI-569` defines media delivery
- `PBI-578` defines bulk target and selection semantics
- `PBI-570` defines the frontend rebuild program and child-PBI order
- `PBI-588` locks the frontend rebuild contract
- `PBI-589` moves the legacy frontend out of the active source tree
- `PBI-590` defines how rebuilt slices are checked against legacy visuals and behavior
- `PBI-571`, `PBI-581` through `PBI-596` execute the frontend rebuild in slices

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This program index is complete only when:
- the frontend reset is explicitly a rebuild, not another in-place migration
- the legacy frontend and the rebuilt frontend have separate ownership boundaries
- each major frontend rebuild problem has its own executable child PBI
- equivalent UI parts are allowed and expected to be merged instead of being preserved as separate legacy-shaped components
- parity/reference checkpoints are explicit
- the activation order is explicit
- dependencies on backend and media delivery are explicit
- the two-track model and activation gates are explicit
