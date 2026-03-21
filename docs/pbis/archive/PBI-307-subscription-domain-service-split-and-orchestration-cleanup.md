# PBI-307: Subscription domain service split and orchestration cleanup

## Priority
P1

## Audit Status (2026-03-15)
Status: **Implemented**

Completion Notes:
1. Subscription CRUD/reset behavior now lives in `./core/src/subscriptions/controller.rs`.
2. Subscription run/stop/query execution orchestration is isolated in `./core/src/subscriptions/run_orchestrator.rs`.
3. Group run/stop orchestration is isolated in `./core/src/subscriptions/group_orchestrator.rs`, and `./core/src/subscriptions/subscription_group_controller.rs` now owns CRUD/schedule state only.
4. Runtime task publication and runtime progress views are separated into:
   - `./core/src/subscriptions/runtime_tasks.rs`
   - `./core/src/subscriptions/progress.rs`
5. Sync/import policy glue that did not belong in the engine is now isolated into:
   - `./core/src/subscriptions/policy.rs`
   - `./core/src/subscriptions/import_policy.rs`
   - `./core/src/subscriptions/archive.rs`
6. `./core/src/subscriptions/sync_engine.rs` remains the query execution engine, but no longer owns group orchestration, archive-prefix ownership, or runtime-task shaping.

## Problem
The subscription domain has no clean internal layering. Controller, engine, and runtime task behavior are mixed across large files. This makes subscription behavior hard to test, hard to evolve, and too tightly coupled to UI expectations.

## Scope
- `core/src/subscriptions/controller.rs`
- `core/src/subscriptions/sync_engine.rs`
- `core/src/subscriptions/subscription_group_controller.rs`
- supporting subscription-related SQLite paths where needed

## Implementation
1. Define explicit subscription-domain layers:
   - CRUD/config service
   - run orchestration service
   - query execution engine
   - metadata merge/dedupe policy helpers
   - runtime task adapter
2. Move archive reset, query naming, and resume policy into dedicated helpers.
3. Make run/stop/reset behavior go through one orchestrator.
4. Remove UI-shaped progress ownership from the sync engine.

## Acceptance Criteria
1. Subscription controller no longer owns both CRUD and run orchestration in one module.
2. Sync engine no longer owns unrelated metadata/runtime policy glue.
3. Reset/resume/cancel semantics are explicit and isolated.
4. Subscription behavior can be tested at service boundaries without renderer assumptions.

## Test Cases
1. Run subscription, cancel, reset, and rerun with consistent state transitions.
2. Resume logic works per query without controller duplication.
3. Inbox-full pause behavior remains correct.

## Risk
High. High-traffic domain with many user-visible code paths.
