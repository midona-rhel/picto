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
- [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)
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

The rebuilt sidebar should treat its repeated UI honestly:
- folder rows, smart-folder rows, and system rows are one sidebar row family with controlled variants
- tree nesting, collapse affordance, drag target state, and selection state are row behaviors, not reasons to fork separate component hierarchies

This slice does not own:
- the rebuilt grid screen
- the rebuilt inspector
- media/viewer logic
- long-term manager navigation and manager-surface ownership for tags, duplicates, and subscriptions
- the long-term random active-image view contract

## Required shape
- a clean new app shell root in `src/app/**`
- a rebuilt sidebar feature in `src/features/sidebar/**`
- new state ownership under `src/state/**` for shell/sidebar concerns
- controller-owned actions for folder/smart-folder/sidebar interactions
- runtime settle path for sidebar count/tree refresh
- one shared sidebar row primitive or row family, not separate legacy-style row implementations for each sidebar subsection
- styling for the rebuilt shell/sidebar follows the CSS architecture contract in [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)

Follow-up work that should not be treated as hidden scope here:
- manager navigation and manager surfaces belong to [PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md](./docs/pbis/active-alpha/PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md)
- the random active-image view contract belongs to [PBI-596-greenfield-random-active-image-view-contract.md](./docs/pbis/active-alpha/PBI-596-greenfield-random-active-image-view-contract.md)

## Start gate
This PBI may start only when:
- [PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md](./docs/pbis/active-alpha/PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md) is review-clean
- [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md) is `Implemented`
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md), [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md), and [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md) are `Implemented` enough for the shell/sidebar slice
- the rebuilt shell/sidebar path does not require imports from legacy runtime/store/controller code

## Next rule
Do not start [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md) until this PBI is `Activated`.

## Acceptance criteria
- the rebuilt shell/sidebar runs as the active path
- the rebuilt shell/sidebar does not import legacy runtime/store/controller modules
- parity is confirmed against the reference harness
- the rebuilt shell/sidebar keeps current visuals and interaction behavior closely enough for product continuity
- repeated sidebar UI parts are merged into one canonical row family where behavior is materially the same
- finishing this PBI means the rebuilt shell/sidebar slice becomes the active default path before the next rebuilt live slice starts
- temporary TODOs are allowed only for cross-PBI boundaries already named in the dependency list; they do not allow the next Track A PBI to start early

## Tests
- shell/sidebar fixture rendering tests
- sidebar interaction tests
- parity checklist and visual confirmation notes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
