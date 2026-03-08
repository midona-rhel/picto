# PBI-349: Backend monolith tail breakup and deletion campaign

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented**

## Problem
Several backend monoliths are too large to remain canonical even after ownership is clarified. Some original flat-root monoliths have already been moved or renamed, but the underlying oversize modules still exist under new paths.

## Scope
- `core/src/subscriptions/gallery_dl_runner.rs`
- `core/src/subscriptions/controller.rs`
- `core/src/subscriptions/sync_engine.rs`
- `core/src/grid/query.rs`
- `core/src/grid/metadata.rs`
- `core/src/media_processing/mod.rs`
- `core/src/sqlite/schema.rs`
- `core/src/sqlite/mod.rs`
- `core/src/runtime_state.rs`
- `core/src/runtime_contract/mod.rs`

Evidence:
1. The original flat-root monolith file names from the audit are mostly gone, which means the PBI scope needs to track current canonical paths instead of old ones.
2. `subscriptions/gallery_dl_runner.rs`, `subscriptions/controller.rs`, `subscriptions/sync_engine.rs`, `sqlite/schema.rs`, `sqlite/mod.rs`, and `runtime_state.rs` are still large enough to justify further breakup.

## Implementation
1. Split each monolith by actual ownership.
2. Delete the original broad file once the split lands.
3. Reject partial refactors that leave giant compatibility shells behind.
4. Track deletion counts as part of the campaign.

## Acceptance Criteria
1. The scoped monolith files are either gone or materially reduced to a justified shell.
2. Canonical logic lives in smaller ownership-correct modules.
3. The deletion campaign removes a meaningful amount of backend LOC.
