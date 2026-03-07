# PBI-308: PTR domain decomposition and runtime state cleanup

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/ptr_controller.rs` is 1000+ lines and owns sync orchestration, bootstrap state, cancellation, startup maintenance, progress emission, and snapshot access.
2. `core/src/ptr_sync.rs` is 1200+ lines and owns chunk orchestration, write timing, progress updates, and engine behavior.
3. `core/src/sqlite_ptr/bootstrap.rs` and `sqlite_ptr/sync.rs` are both 1000+ lines and represent substantial subdomains inside PTR storage.
4. PTR runtime state is still managed through global flags and mutexes local to the controller.

## Problem
PTR is functionally a separate subsystem inside the backend, but it still relies on controller-centric global runtime state and oversized modules. Sync, bootstrap, overlay, cache, and storage concerns are not separated cleanly enough for long-term maintainability.

## Scope
- `core/src/ptr_controller.rs`
- `core/src/ptr_sync.rs`
- `core/src/ptr_client.rs`
- `core/src/ptr_types.rs`
- `core/src/sqlite_ptr/*`

## Implementation
1. Split PTR into clearer layers:
   - runtime/orchestration
   - sync engine
   - bootstrap/import engine
   - overlay/cache services
   - client/protocol layer
2. Move PTR runtime state into the shared runtime/task model.
3. Reduce controller-local globals and mutexes.
4. Clarify ownership between controller, sync engine, and SQLite PTR storage modules.

## Acceptance Criteria
1. PTR runtime state is no longer controller-local global state.
2. Sync/bootstrap/cache/overlay responsibilities are clearer.
3. PTR modules are easier to navigate and test.
4. Progress reporting and cancellation integrate with the shared runtime model.

## Test Cases
1. PTR sync start/cancel/restart still work.
2. Bootstrap and compact build still report progress correctly.
3. Overlay reads remain correct during and after sync.

## Risk
High. PTR is large and correctness-sensitive.
