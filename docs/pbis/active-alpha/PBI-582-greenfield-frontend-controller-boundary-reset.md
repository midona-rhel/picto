# PBI-582: Greenfield frontend controller boundary reset

## Priority
P1

## AI-generated caveat
This document is about controller ownership. It is not the API-layer PBI, not the frontend state-ownership PBI, and not the shared-component PBI.

## Lifecycle
- `Implemented` when domain controllers are the clear owners of backend-facing frontend behavior for the intended slice.
- `Activatable` when `PBI-581` is implemented and the intended backend commands exist.
- `Activated` when the intended user flows use controller-owned paths by default.
- `Legacy removed` when replaced feature-local backend behavior paths are deleted.

Activation depends on:
- [PBI-581-greenfield-frontend-api-layer-reset.md](./docs/pbis/active-alpha/PBI-581-greenfield-frontend-api-layer-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)

## Problem
Controller ownership is still uneven and too much behavior is spread across features and hooks.

Current problems:
- some domains still split single-item and multi-item flows unnecessarily
- command semantics are duplicated across controllers, hooks, and feature code
- undo/redo ownership is not consistently controller-owned
- controllers are not yet the obvious place to look for domain behavior
- controllers and state ownership are still blurred in some slices
- some controllers still know too much about UI-side stores and eager patching details
- the current tree still encourages controllers to patch around legacy architectural damage instead of fronting a clean rebuilt UI

## Product model to encode
Controllers should own:
- backend calls
- target building
- optimistic local updates where appropriate
- undo/redo registration
- domain-level command consolidation

Feature code should not own those responsibilities.
Controllers should not become the long-term owner of all visible state either. That belongs in the state-ownership layer for migrated slices.
Controllers also should not know several frontend store internals just to make one action appear correct.

## Required shape
- one clear controller surface per domain
- controller methods describe domain actions, not transport mechanics
- controllers depend on the API layer, not raw transport
- feature code consumes controllers/view-model hooks only
- controllers write one intended state path per migrated slice
- rebuilt controllers do not coordinate with legacy store implementations as a normal behavior path

## Implementation changes
- collapse repeated single-vs-batch flows into one controller path where possible
- move target construction out of feature code
- remove duplicated command semantics from hooks and feature components
- make controller boundaries explicit in the main frontend domains
- remove controller knowledge of several store implementations for the same migrated slice

## Acceptance criteria
- intended frontend slices use controllers as the only backend-facing layer
- duplicated feature-level command logic is materially reduced
- controller method names are domain-led and clear
- undo/redo ownership is controller-led where applicable
- controllers for activated slices are action-only and no longer serve as store patch coordinators

## Tests
- controller behavior tests for migrated domains
- boundary tests proving feature code is not issuing backend calls directly
- undo/redo ownership tests where applicable

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
