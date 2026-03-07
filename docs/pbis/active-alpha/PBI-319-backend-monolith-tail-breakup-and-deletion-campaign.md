# PBI-319: Backend monolith tail breakup and deletion campaign

## Priority
P1

## Problem
Several backend monoliths are too large to remain canonical even after ownership is clarified. If they are only moved, not broken up and deleted, the architecture does not improve.

## Scope
- `core/src/gallery_dl_runner.rs`
- `core/src/subscription_controller.rs`
- `core/src/subscription_sync.rs`
- `core/src/grid_controller.rs`
- `core/src/files/mod.rs`
- `core/src/sqlite/schema.rs`
- `core/src/sqlite/mod.rs`
- `core/src/sqlite_ptr/mod.rs`
- `core/src/runtime_state.rs`
- `core/src/runtime_contract/mod.rs`

## Implementation
1. Split each monolith by actual ownership.
2. Delete the original broad file once the split lands.
3. Reject partial refactors that leave giant compatibility shells behind.
4. Track deletion counts as part of the campaign.

## Acceptance Criteria
1. The scoped monolith files are either gone or materially reduced to a justified shell.
2. Canonical logic lives in smaller ownership-correct modules.
3. The deletion campaign removes a meaningful amount of backend LOC.
