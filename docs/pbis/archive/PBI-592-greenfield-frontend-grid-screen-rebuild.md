# PBI-592: Greenfield frontend grid screen rebuild

## Priority
P1

## AI-generated caveat
This document is about rebuilding the grid screen as a clean surface in the new frontend. It is not an in-place refactor of `ImageGrid.tsx`.

## Lifecycle
- `Implemented` when the rebuilt grid screen exists as a clean feature root in the new `src/**` tree.
- `Activatable` when canonical query/selection contracts are locked and the live path is review-clean.
- `Activated` when the rebuilt grid screen is the live path.
- `Legacy removed` when the legacy grid-screen path is deleted.

Activation depends on:
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-597-greenfield-entity-view-reconciliation-and-sidebar-delta-contract.md](./docs/pbis/active-alpha/PBI-597-greenfield-entity-view-reconciliation-and-sidebar-delta-contract.md)

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
- one stable right-hand inspector rail with its own titlebar segment

The rebuilt grid screen should not treat runtime changes as “always refetch the whole current grid”.
It should:
- ignore unrelated state-change events
- patch simple visible-row updates when possible
- ask the backend to reconcile the current query/window when membership or ordering may have changed

The rebuilt grid screen should also avoid inventing its own isolated visual primitives when equivalent ones already exist elsewhere in the rebuilt frontend.
If the grid tile preview is the same rounded media preview used by inspector or viewer-adjacent UI, it should be one shared preview family with small variants.
Its styling should follow the CSS architecture contract in [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md) instead of recreating feature-owned CSS for shared primitives.

The rebuilt grid screen should not depend on:
- the legacy `ImageGrid.tsx` architecture
- legacy slim-grid contracts
- giant prop bags from the app shell

## Start gate
This PBI may start only when:
- [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md) is `Activated`
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md), [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md), and [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md) are `Implemented` and review-clean for the grid path
- grid live-path verification is complete enough to compare real grid states

## Next rule
Do not start [PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md](./docs/pbis/active-alpha/PBI-593-greenfield-frontend-inspector-and-selection-surface-rebuild.md) until this PBI is `Activated`.

## Acceptance criteria
- the rebuilt grid screen is a clean feature root
- canonical query/selection semantics are used by default
- selection-driven actions target what the user actually selected
- the activated grid screen includes a stable right-hand inspector/context rail
- the live grid path is review-clean
- the rebuilt grid no longer depends on the legacy `ImageGrid.tsx` architecture
- tile/chrome primitives that are visually/functionally equivalent to inspector or preview surfaces are shared instead of re-implemented
- grid runtime settle is query-aware and does not rely on broad refetch as the normal correctness path
- finishing this PBI means the rebuilt grid slice becomes the active default path before the next rebuilt live slice starts
- temporary TODOs are allowed only for cross-PBI boundaries already named in the dependency list; they do not allow the next Track A PBI to start early

## Tests
- query construction tests
- selection-target translation tests
- live-path verification notes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
