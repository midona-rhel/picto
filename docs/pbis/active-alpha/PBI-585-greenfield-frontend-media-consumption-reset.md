# PBI-585: Greenfield frontend media consumption reset

## Priority
P1

## AI-generated caveat
This document is specifically about how the frontend consumes thumbnails, previews, originals, and streams. It is not the same thing as the backend media-delivery PBI.

## Lifecycle
- `Implemented` when the frontend has one clear media-consumption layer for the intended slice.
- `Activatable` when `PBI-569`, `PBI-581`, and `PBI-583` are implemented enough for the intended viewer/grid/preview flow.
- `Activated` when migrated frontend media surfaces use media delivery outputs by default.
- `Legacy removed` when replaced path-based media helper usage is deleted for that slice.

Activation depends on:
- [PBI-569-greenfield-media-delivery-service.md](./docs/pbis/active-alpha/PBI-569-greenfield-media-delivery-service.md)
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-583-greenfield-frontend-runtime-reconciliation-reset.md](./docs/pbis/active-alpha/PBI-583-greenfield-frontend-runtime-reconciliation-reset.md)

## Problem
Frontend media consumption still carries too many path-shaped and implementation-shaped assumptions.

Current problems:
- viewer/grid/preview surfaces are not cleanly centered on one media-consumption model
- path helpers and transport-shaped assumptions still leak upward
- media-role semantics are not consistently expressed in the frontend

## Product model to encode
Frontend media consumption should:
- use stable media URLs or handles
- think in asset roles, not paths
- consume one consistent collection primary-member rule
- keep media display concerns out of unrelated feature logic

## Implementation changes
- introduce one frontend media-consumption helper layer if needed
- migrate viewer/grid/preview surfaces to media URLs/handles
- remove path-based assumptions from migrated slices

## Acceptance criteria
- migrated media surfaces consume media delivery outputs by default
- path-shaped media assumptions are removed from those slices
- collection media behavior is consistent across migrated surfaces

## Tests
- media-role consumption tests
- viewer/grid/preview integration tests
- boundary tests proving migrated slices do not depend on filesystem-path helpers

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
