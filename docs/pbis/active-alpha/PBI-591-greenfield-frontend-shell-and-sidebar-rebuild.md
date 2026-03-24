# PBI-591: Greenfield frontend shell and sidebar rebuild

## Priority
P1

## AI-generated caveat
This document is about the first rebuilt live surface: app shell and sidebar. It should be the first real new frontend slice because it is broad enough to prove the rebuild shape, but smaller and safer than the grid.

## Lifecycle
- `Implemented` when the rebuilt shell and sidebar exist in the new `src/**` tree with their own state, controller, and runtime boundaries.
- `Activatable` when parity fixtures exist and the rebuilt shell/sidebar no longer depend on legacy frontend runtime/store/controller code.
- `Activated` when the app uses the rebuilt shell/sidebar by default.
- `Legacy removed` when the matching legacy shell/sidebar slice is deleted.

Activation depends on:
- [PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md](./docs/pbis/active-alpha/PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md)
- [PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md](./docs/pbis/active-alpha/PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)

## Problem
The current app shell and sidebar are tightly tied to the old frontend architecture. Rebuilding the shell/sidebar first gives the new frontend a real home without immediately fighting the full grid.

## Product model to encode
This slice owns:
- app shell layout
- titlebar/window controls composition
- sidebar system scopes
- folder tree
- smart folder tree
- tag/duplicate/system counts shown in the sidebar
- shell-level navigation entry into the main surfaces

This slice does not own:
- the rebuilt grid screen
- the rebuilt inspector
- media/viewer logic

## Required shape
- a clean new app shell root in `src/app/**`
- a rebuilt sidebar feature in `src/features/sidebar/**`
- new state ownership under `src/state/**` for shell/sidebar concerns
- controller-owned actions for folder/smart-folder/sidebar interactions
- runtime settle path for sidebar count/tree refresh

## Acceptance criteria
- the rebuilt shell/sidebar runs as the active path
- the rebuilt shell/sidebar does not import legacy runtime/store/controller modules
- parity is confirmed against the reference harness
- the rebuilt shell/sidebar keeps current visuals and interaction behavior closely enough for product continuity

## Tests
- shell/sidebar fixture rendering tests
- sidebar interaction tests
- parity checklist and visual confirmation notes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
