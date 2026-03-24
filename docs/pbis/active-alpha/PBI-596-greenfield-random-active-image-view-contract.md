# PBI-596: Greenfield random active-image view contract

## Priority
P2

## AI-generated caveat
This document is about the random view specifically. It exists because random is not just another sidebar label. It needs product and contract decisions around source set, stability, back/forward behavior, and query integration.

## Lifecycle
- `Implemented` when the random active-image view contract and frontend/backend boundaries are written clearly enough to implement without inventing behavior during coding.
- `Activatable` when the rebuilt grid/query path can support the random contract cleanly.
- `Activated` when the rebuilt frontend can open and navigate the random active-image view by default.
- `Legacy removed` when any old ad hoc random-entry workaround is deleted.

Activation depends on:
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md)
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md)

## Problem
Random should not be treated as a fake manager entry or a hand-waved sidebar item. It is a real product behavior that affects query semantics and navigation.

## Product model to encode
Lock these decisions:
- random draws from `active` library items only
- user-facing label may be `Random`
- the view must be stable enough for back/forward navigation
- stability comes from a seed/hash owned by the route/query state
- the implementation is query-based, not a separate unrelated data source

The intended model is:
- start from the active-image query set
- apply a seeded random ordering or seeded random subset selection
- carry the random seed/hash through navigation so the same random result set can be revisited

## Required shape
- one explicit random-view contract in the rebuilt frontend/backend boundary
- route/query state carries the random seed/hash
- random integrates with the rebuilt grid/query model instead of bypassing it
- random remains clearly separate from manager navigation

## Open product defaults locked here
- source set: active images only
- label: `Random`
- navigation requirement: back/forward must restore the same random view
- implementation direction: use a query option / ordering mode plus stable seed/hash

## Start gate
This PBI may start only when:
- [PBI-584-greenfield-frontend-grid-query-and-selection-reset.md](./docs/pbis/active-alpha/PBI-584-greenfield-frontend-grid-query-and-selection-reset.md) is review-clean for the rebuilt grid path
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md) is `Activated`

## Next rule
Do not keep `Random` as a frontend-only sidebar exception once this PBI starts.
Implement it as a proper query/navigation contract or leave it out of the rebuilt live path until it exists.

## Acceptance criteria
- random uses active images only
- random has a stable seed/hash for back/forward navigation
- random is integrated with the rebuilt grid/query model
- random is not modeled as a manager surface
- the implementation is simpler and more explicit than the old ad hoc approach

## Tests
- random query-contract tests
- navigation tests proving seed/hash stability across back/forward
- selection/bulk-target tests if random view participates in bulk actions

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
