# PBI-581: Greenfield frontend API layer reset

## Priority
P1

## AI-generated caveat
This document focuses on `src/platform/**`, typed command/event contracts, and the frontend-facing backend adapter surface. It is not the controller PBI and not the runtime PBI.

## Lifecycle
- `Implemented` when the frontend API layer is a clear transport adapter boundary instead of a loose pile of invoke helpers.
- `Activatable` when `PBI-568` canonical backend commands exist for the intended slice.
- `Activated` when the intended frontend flows use the new API layer by default.
- `Legacy removed` when replaced raw transport/old command-name helpers for that slice are deleted.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)

## Problem
The frontend API layer is still too close to raw transport and old command shapes.

Current problems:
- `src/platform/api.ts` is too broad and mixes multiple generations of backend contract
- old and new command names live side by side without one stable frontend API model
- target conversion and selection conversion are not cleanly owned
- typed command maps still expose legacy names and shapes next to new ones

## Product model to encode
The frontend API layer should:
- be the only place that knows transport command names
- expose one typed frontend-facing backend adapter contract
- own request/response normalization between transport and frontend model
- stop leaking old transport names into controllers and features

## Required shape
- one clear API-layer boundary under `src/platform/**`
- command-name ownership lives there, not in controllers/features
- typed contract files are treated as transport types, not frontend domain truth
- target-building helpers live here or in one dedicated frontend adapter helper layer, not scattered across features

## Implementation changes
- reduce `src/platform/api.ts` into clearer submodules if needed
- separate canonical API methods from legacy compatibility methods
- centralize `EntityTarget`/query conversion in one owned place
- remove direct `invoke` usage outside the API layer

## Acceptance criteria
- frontend code outside the API layer does not know transport command names
- canonical backend commands have first-class API methods
- target/query conversion is centralized
- legacy API shims are clearly isolated and removable

## Tests
- API-layer contract tests for canonical command payloads
- serialization/normalization tests for request and response shapes
- boundary tests proving feature/controller code does not call raw transport directly

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
