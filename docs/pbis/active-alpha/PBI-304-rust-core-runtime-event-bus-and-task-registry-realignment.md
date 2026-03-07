# PBI-304: Rust core runtime event bus and task registry realignment

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/events.rs` emits `state-changed`, `sidebar-invalidated`, and `grid-snapshot-invalidated` as UI-facing concepts rather than domain/runtime concepts.
2. `core/src/subscription_sync.rs` owns its own in-memory runtime progress map via `SUB_RUNTIME_PROGRESS`.
3. `core/src/ptr_controller.rs` owns separate global sync/bootstrapping flags, progress state, and cancellation tokens.
4. `core/src/flow_controller.rs` emits its own progress lifecycle independently of the subscription and PTR runtime paths.
5. Sequence handling exists only for `state-changed`, not for the runtime task surface as a whole.

## Problem
The Rust core has no single runtime event bus or task registry. Mutation notifications, long-running task progress, and pollable fallback state are each implemented differently depending on domain. This makes the backend hard to reason about and directly causes renderer desynchronization after reloads, listener loss, or parallel background work.

## Scope
- `core/src/events.rs`
- `core/src/state.rs`
- `core/src/subscription_sync.rs`
- `core/src/subscription_controller.rs`
- `core/src/flow_controller.rs`
- `core/src/ptr_controller.rs`
- `core/src/ptr_sync.rs`

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
