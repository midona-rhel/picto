# PBI-304: Rust core runtime event bus and task registry realignment

## Priority
P1

## Audit Status (2026-03-07)
Status: **Partially Implemented**

### What's done:
1. `core/src/runtime_state.rs` — centralized task registry with `upsert_task`, `remove_task`, `get_runtime_snapshot`. Monotonic sequence counter shared with `events.rs`.
2. `core/src/runtime_contract/` — typed contract types (`MutationReceipt`, `RuntimeTask`, `RuntimeSnapshot`, `TaskKind`, `TaskStatus`, `TaskProgress`) with ts-rs TypeScript generation.
3. `core/src/events.rs` — `emit_mutation()` produces sequenced `MutationReceipt` events. Legacy `state-changed` / `sidebar-invalidated` / `grid-snapshot-invalidated` events fully removed.
4. All three task families (subscriptions, flows, PTR) publish through `runtime_state::upsert_task`. `SUB_RUNTIME_PROGRESS` parallel state removed — subscription progress is now stored in `RuntimeTask.detail`.
5. Frontend `runtimeSyncStore.ts` subscribes to `runtime/mutation_committed`, `runtime/task_upserted`, `runtime/task_removed`.
6. Snapshot recovery via `get_runtime_snapshot` dispatch command wired end-to-end.

### What remains:
1. PTR controller still maintains operational `AtomicBool` guard flags (`PTR_SYNCING`, `PTR_BOOTSTRAP_RUNNING`, `PTR_COMPACT_BUILD_RUNNING`) for mutual exclusion. These serve a different purpose than progress reporting but represent parallel state.
2. Legacy domain-specific events (`subscription-started`, `flow-started`, `ptr-sync-started`, etc.) still emitted alongside `upsert_task` for frontend backward compatibility. Frontend listeners should migrate to task-based model.
3. Task events (`runtime/task_upserted`, `runtime/task_removed`) are not sequenced — only mutation events carry sequence numbers.

## Problem
The Rust core has no single runtime event bus or task registry. Mutation notifications, long-running task progress, and pollable fallback state are each implemented differently depending on domain. This makes the backend hard to reason about and directly causes renderer desynchronization after reloads, listener loss, or parallel background work.

## Scope
- `core/src/events.rs`
- `core/src/runtime_state.rs`
- `core/src/runtime_contract/` (mutation, task, snapshot)
- `core/src/subscriptions/controller.rs`
- `core/src/subscriptions/sync_engine.rs`
- `core/src/subscriptions/flow_controller.rs`
- `core/src/ptr/controller.rs`
- `core/src/ptr/sync_engine.rs`

## Dependencies
Depends on:
1. `PBI-302` for the fact-level mutation receipt shape.

Feeds:
1. `PBI-303` for deterministic resource invalidation from runtime facts.
2. renderer/runtime store cutover work under `PBI-234`.

## Not In Scope
1. Defining the business meaning of scopes like `system:all`, `untagged`, or `uncategorized`.
2. Rewriting grid/selection/sidebar query logic.
3. Frontend-side resource dependency rules.

## Implementation
1. Introduce a backend runtime registry module that owns:
   - monotonic runtime sequence numbers
   - active task snapshots
   - typed mutation receipts
   - runtime snapshot queries
2. Split runtime events into two primary families:
   - mutation receipts
   - task upsert/remove events
3. Move subscription, flow, and PTR progress ownership into the runtime registry.
4. Keep legacy events only as compatibility shims during migration.
5. Add snapshot APIs so the renderer can recover task state after reload.

## Acceptance Criteria
1. Runtime task state is owned by one backend subsystem.
2. Subscriptions, flows, and PTR all publish through the same task model.
3. Runtime events are sequenced and snapshot-recoverable.
4. `events.rs` no longer encodes renderer-specific invalidation as its primary contract.

## Test Cases
1. Start a subscription, reload the renderer, and confirm task snapshot recovery works.
2. Run PTR sync and confirm runtime task state remains available through snapshot + events.
3. Verify old compatibility events still fire during migration.

## Risk
High. This touches the backend runtime contract and several long-running worker paths simultaneously.
