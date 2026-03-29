# PBI-584: Greenfield frontend grid query and selection reset

## Priority
P1

## AI-generated caveat
This document is about the canonical query/selection/data contract for the rebuilt grid path. It is not the visual/UI rebuild PBI for the grid screen itself.

## Lifecycle
- `Implemented` when the frontend app-shell/grid/query/selection path has one clear canonical model in code.
- `Activatable` when `PBI-568`, `PBI-578`, `PBI-581`, `PBI-583`, `PBI-587`, and `PBI-588` are implemented enough for the real grid/query/selection flow.
- `Activated` when the live grid and selection-driven entity actions use the canonical query/target model by default.
- `Legacy removed` when replaced slim/grid-page selection paths for that slice are deleted.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)

## Problem
The app-shell/grid/query/selection path is still split between old view models and new canonical backend semantics.

Current problems:
- `App.tsx` still assembles too much grid-related state
- `MainViewRouter.tsx` still passes a very wide grid prop surface
- `ImageGrid.tsx` is still the place where route input, query building, pagination, selection, viewer bridging, and rendering meet
- `get_grid_page_slim` still anchors important frontend behavior
- selection semantics still carry old `SelectionQuerySpec` behavior that does not line up cleanly with canonical backend targets
- bulk actions are still easy to get wrong when they originate from filtered or virtual grid selections
- grid-visible state and selection state are still not clearly owned enough
- runtime/grid settle still lacks a clean answer for “does the current query/window change?”
- the frontend still cannot know final placement/membership of changed entities from event hashes alone

## Product model to encode
The rebuilt grid path should:
- build one canonical typed entity-view query
- drive selection actions through the same canonical target semantics
- keep top-level grouped grid semantics aligned with what backend bulk actions will target
- provide the stable data/selection contract that the rebuilt grid UI can depend on
- keep the current `EntityViewQuery` in frontend-owned state
- treat visible-window settlement as backend-owned truth via a typed reconcile call, not frontend guesswork
- expose `total_size_bytes` alongside `total_count` for scope-aware inspector surfaces
- keep inspector visibility separate from selection ownership
- let the inspector read scope context from canonical grid state plus sidebar state
- keep inspector display state committed, not derived directly from raw live navigation during transitions

Locked decision:
- the backend should not generally track the frontend’s live grid/session state
- the frontend owns the current query, current visible window/anchor state, and back/forward history
- the backend owns whether a change affects that query and how the visible window should settle

The visual/UI rebuild of the grid screen is tracked separately in:
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md)

## Implementation changes
- move broad grid assembly out of `App.tsx`
- stop passing the current giant prop bag from `MainViewRouter.tsx` into `ImageGrid`
- split the current grid into:
  - screen/root composition
  - query input
  - selection
  - results/pagination
  - viewer bridge
  - runtime settle adapter
  - renderer/presentational layer
- migrate frontend grid data flow away from slim/grid-page legacy contract
- align virtual selection and select-all behavior with canonical backend targets
- remove duplicated selection translation logic where possible
- make grouped top-level selection semantics explicit and testable
- define the reconcile contract between current query/window state and backend state-change facts
- keep bulk summary/read helpers on the same canonical `EntityTarget` model as bulk writes
- do not let select-all summary behavior fall back to loaded-window reductions for tags/folders/rating/size

## Acceptance criteria
- the rebuilt grid data path uses the canonical entity-view query model
- selection-driven entity actions target what the user actually selected
- virtual selection/select-all semantics are stable and explicit
- virtual selection/select-all summary semantics are DB-owned and truthful, not loaded-window approximations
- slim/grid-page legacy dependence is materially reduced or removed for the rebuilt slice
- the rebuilt grid UI can depend on this contract without legacy translation helpers
- current query state, visible-window state, inspector display-state semantics, and backend reconcile semantics are explicit and testable

## Tests
- app-shell to grid-root boundary tests
- grid query construction tests
- selection-target translation tests
- grouped selection behavior tests
- select-all summary tests for shared tags/folders/rating without hash expansion
- integration tests for filtered selection -> action -> runtime settle
- reconcile-decision tests for current query vs state-change scope impact

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
