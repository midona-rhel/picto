# PBI-595: Greenfield frontend manager navigation and surface reset

## Priority
P1

## AI-generated caveat
This document is about the frontend manager surfaces that sit alongside the library scopes. It exists because tags, duplicates, and subscriptions are manager-style tools, not normal entity-view scopes, and should not be carried as ad hoc sidebar exceptions.

## Lifecycle
- `Implemented` when the rebuilt frontend has one clear manager-navigation model and simplified manager surfaces for the intended slice.
- `Activatable` when the rebuilt shell/sidebar is live and the rebuilt manager entries no longer depend on legacy runtime/store/controller modules.
- `Activated` when manager navigation and the migrated manager surfaces use the new path by default.
- `Legacy removed` when the matching legacy manager navigation and manager-surface paths are deleted.

Activation depends on:
- [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md)

## Problem
Tags, duplicates, and subscriptions are manager-style tools. They are not normal library scopes, and they should not be modeled as if they were just more sidebar entity filters.

Current problems:
- manager entries are mixed into scope/navigation handling
- manager surfaces carry more UI and state duplication than they need
- tags and duplicates already behave like managers, while subscriptions is expected to join the same group
- the rebuilt shell/sidebar otherwise risks inventing temporary frontend-only sidebar entries instead of using a clear manager-navigation model

## Product model to encode
This PBI should establish:
- a clear distinction between `library scopes` and `manager entries`
- one manager-navigation group in the rebuilt shell/sidebar
- dedicated manager surfaces for:
  - tags manager
  - duplicates manager
  - subscriptions manager
- simplified shared UI and state structure across those manager surfaces where the behavior is materially the same

The clean rule is:
- scopes change the entity/grid view
- managers open dedicated management surfaces

## Required shape
- rebuilt shell/sidebar treats manager entries as a separate group from library scopes
- manager entries are not invented as fake scope nodes in the backend sidebar contract
- manager surfaces use the rebuilt controller/state/runtime boundaries
- repeated manager UI patterns are simplified aggressively instead of being rebuilt as separate legacy-shaped surfaces

Examples of expected consolidation:
- manager headers and actions should share one manager-shell pattern
- manager list/table/filter scaffolding should be shared where behavior is materially the same
- tags, duplicates, and subscriptions should not each invent their own unrelated panel/chrome system if one family is enough

## Start gate
This PBI may start only when:
- [PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md](./docs/pbis/active-alpha/PBI-591-greenfield-frontend-shell-and-sidebar-rebuild.md) is `Activated`
- the rebuilt shell/sidebar path is stable enough to host a dedicated manager-entry group
- the required manager API/controller/state boundaries are `Implemented` and review-clean for the intended manager slice

## Next rule
Manager entries and manager surfaces may activate surface-by-surface.
Do not fold manager-navigation exceptions back into `PBI-591`; use this PBI for that work.

## Acceptance criteria
- manager entries are clearly distinct from library scopes
- tags and duplicates are no longer modeled as fake sidebar scopes
- subscriptions can join the same manager group without a new navigation model
- migrated manager surfaces use rebuilt controller/state/runtime boundaries
- repeated manager UI structure is simplified materially compared with the legacy frontend

## Tests
- manager-navigation rendering tests
- manager surface smoke/regression tests
- parity notes for the migrated manager surfaces
- boundary checks proving manager entries are not treated as entity-view scopes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
