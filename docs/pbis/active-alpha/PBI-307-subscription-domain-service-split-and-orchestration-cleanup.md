# PBI-307: Subscription domain service split and orchestration cleanup

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. Subscription code has been folderized into `core/src/subscriptions/`, but `core/src/subscriptions/controller.rs` still mixes CRUD/config and run/reset orchestration.
2. `core/src/subscriptions/sync_engine.rs` still owns sync orchestration, metadata merge, duplicate auto-merge behavior, resume cursors, and runtime task shaping in one engine.
3. `core/src/subscriptions/subscription_group_controller.rs` still carries UI-facing group/run behavior close to orchestration instead of behind a narrower service boundary.
4. Query naming, reset semantics, completion semantics, and inbox-full behavior are still spread across controller and sync-engine layers.

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
