# PBI-597: Greenfield entity-view reconciliation and sidebar delta contract

## Priority
P1

## AI-generated caveat
This document locks the missing reactive contract between backend truth, runtime events, sidebar settle, and the rebuilt grid. It is intentionally specific because broad invalidation and vague event handling are currently the main architectural gap.

## Lifecycle
- `Implemented` when the backend/runtime/frontend contract for query reconciliation and sidebar deltas exists in code and is documented clearly.
- `Activatable` when `PBI-568`, `PBI-581`, `PBI-583`, `PBI-584`, and `PBI-587` are implemented enough for the intended shell/grid slice.
- `Activated` when live rebuilt sidebar/grid flows reconcile through this contract by default.
- `Legacy removed` when broad fallback sidebar/grid invalidation is no longer the normal correctness path for the activated slice.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)

## Problem
The current reactive model is still too crude:
- the sidebar often treats state changes as “fetch the whole tree again”
- the grid often treats any changed hash as “reload the whole current grid”
- `entity_hashes` alone are not enough to know whether the current query changed
- the frontend cannot infer where a new or updated entity belongs in a deeply sorted/filtered/paged grid
- the backend already knows query truth, but the runtime/frontend contract does not expose that cleanly enough

## Locked decisions

### 1. The backend does not own live frontend session state
The backend should not generally track:
- which window is open
- the user’s back/forward stack
- the currently visible scroll window as durable backend session state

The frontend owns:
- the current `EntityViewQuery`
- current selection state
- current visible window/anchor state needed for reconciliation
- back/forward navigation state

The backend owns:
- query truth
- membership evaluation
- sort/order evaluation
- authoritative change facts

### 2. Runtime events must describe scope impact, not just hashes
`runtime/state_changed` should continue to emit fact fields like:
- `entity_hashes`
- `status_changed`
- `tags_changed`
- `folder_membership_changed`
- `smart_folder_ids`
- `extra_grid_scopes`

But activated slices must treat these as:
- enough to decide whether the current query might be stale
- not enough to place an entity into the current visible grid window by frontend inference alone

### 3. The grid uses backend reconciliation, not frontend guesswork
For query membership/order changes, the normal path should be a backend reconcile call, not broad refetch and not frontend re-sorting from events alone.

Use a typed reconcile operation such as:

```ts
reconcile_entity_view({
  query,
  visible_entity_hashes,
  anchor,
  seq,
})
```

The exact request shape may tighten during implementation, but the returned decision must be one of:
- `no_change`
- `patch_rows`
- `replace_window`
- `full_refresh_required`

The backend is the only layer allowed to decide:
- whether a changed/new entity now belongs to the current query
- where it lands under the current sort
- what visible rows fall out of the current window

### 4. Sidebar settle uses exact deltas by default
The sidebar should stop using “fetch the whole tree” as the normal correctness path.

The runtime/sidebar contract should carry:
- O(1) sidebar counts
- exact node changes when tree structure or node presentation changes
- explicit fallback only when a full refresh is truly required

Examples of exact sidebar delta data:
- changed counts for specific ids
- inserted/removed node ids
- parent changes
- sort-order changes
- node patches for name/icon/color/freshness/selectable fields

Full sidebar-tree fetch remains allowed only as:
- an explicit fallback for broad compiler/projection churn
- an epoch mismatch recovery path
- a temporary bridge while the delta contract is landing

### 5. One merged state-change event per completed action
The normal backend contract for one logical action is:
- compute all committed effects
- merge them into one `ChangeImpact`
- emit one final `runtime/state_changed`

Do not model one logical action as several state-change events just because it touched:
- sidebar deltas
- grid scopes
- entity hashes
- count changes
- multiple backend services

Those facts should be merged into one post-commit event.

Separate later events are correct only for later distinct async phases, such as:
- compiler/projection follow-up work
- smart-folder bitmap/count recomputation
- deferred derivative completion

This PBI should tighten the event contract around richer merged facts, not normalize event spam.

## Product model to encode
The clean reactive split is:
- controllers do the eager local step when appropriate
- runtime receives backend state facts
- state owns the current query/sidebar tree and visible window state
- runtime asks backend to reconcile query-visible state when membership/order is uncertain
- runtime applies exact sidebar deltas directly when available
- runtime keeps displayed scope context aligned with the displayed grid, not raw active-node state during fades

## Implementation changes
- add a typed backend entity-view reconcile contract
- add typed frontend API methods for reconcile calls
- make backend actions aggregate their `ChangeImpact` first and emit one merged `runtime/state_changed` per completed action
- teach runtime/grid settle to compare the current query against event scope hints before deciding to reconcile or ignore
- teach runtime/sidebar settle to apply exact count/node deltas instead of broad tree fetch as the normal path
- keep broad refresh only as explicit fallback, not the default

## Acceptance criteria
- activated grid/sidebar slices no longer use broad invalidation as the normal correctness path
- the backend does not own frontend session/view state
- the frontend does not guess query membership/order from hashes alone
- the grid can ignore unrelated events, patch simple visible-row updates, or reconcile the current window through the backend as appropriate
- the sidebar can apply count/tree deltas directly for normal state changes
- displayed scope inspector content stays on the outgoing grid until the outgoing fade finishes
- entity inspector swaps use one committed displayed snapshot instead of staggered field updates
- one completed backend action normally produces one merged self-describing `runtime/state_changed` event
- full sidebar/grid refresh remains only as a documented fallback path

## Tests
- reconcile decision tests for current query vs state-change facts
- sidebar delta application tests
- integration tests for new entity/status/tag/folder/smart-folder changes against the current grid query
- integration tests for sidebar count/tree changes without broad fetch

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
