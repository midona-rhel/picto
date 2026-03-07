# PBI-306: App state service lifecycle and worker boundary cleanup

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/state.rs` owns the global singleton, library switching, worker spawning, compiler wiring, PTR path selection, cancellation, and scheduler startup.
2. `AppState` currently acts as both domain service container and process lifecycle coordinator.
3. Background worker startup is partially wired directly inside `open_library()`.
4. The compiler loop emits frontend-facing state events directly from the startup path.
5. Library open/close semantics are difficult to test in isolation because service construction and worker orchestration are intertwined.

## Problem
`state.rs` is a service locator plus a process supervisor plus a lifecycle controller. This makes global state brittle, obscures service boundaries, and makes shutdown/restart behavior harder to reason about than it should be.

## Scope
- `core/src/state.rs`
- `core/src/lib.rs`
- worker startup/shutdown paths across the core

## Implementation
1. Split service construction from worker orchestration.
2. Introduce explicit runtime/service containers for:
   - database services
   - background workers
   - cancellation/runtime control
3. Move worker boot logic out of `open_library()` into dedicated lifecycle functions.
4. Ensure library close/reset tears down workers through explicit lifecycle ownership rather than ad hoc globals.

## Acceptance Criteria
1. `state.rs` no longer owns unrelated startup concerns in one file.
2. Worker lifecycle is explicit and testable.
3. Library open/close behavior is deterministic and easier to smoke test.
4. Runtime event emission is not hardwired into state boot logic.

## Test Cases
1. Open library -> start workers -> close library -> reopen library without leaked workers.
2. Cancel running background jobs during close and confirm shutdown is clean.
3. Exercise compiler boot and confirm the system still reaches a consistent ready state.

## Risk
Medium-high. Structural move across core startup code with a high regression surface if done without staged tests.
