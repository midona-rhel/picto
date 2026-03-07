# PBI-305: Derived read-model publish boundary cleanup for SQLite

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/sqlite/compilers.rs`, `bitmaps.rs`, `projections.rs`, and `sidebar.rs` together form a derived read-model system with implicit coupling.
2. Write paths in `sqlite/files.rs`, `sqlite/folders.rs`, `sqlite/tags.rs`, and `sqlite/collections.rs` emit compiler events directly.
3. Compiler completion currently feeds frontend-facing invalidation via `state.rs` and `events.rs`.
4. Published artifacts, bitmap epochs, sidebar projection refresh, and smart-folder rebuilds are coupled but not exposed through one explicit publish boundary.

## Problem
The SQLite derived-read-model system is powerful but too implicit. Domain writes know too much about compiler event details, and publish completion is not represented as a clear backend boundary. This makes read-model invalidation and future runtime synchronization harder than necessary.

## Scope
- `core/src/sqlite/compilers.rs`
- `core/src/sqlite/bitmaps.rs`
- `core/src/sqlite/projections.rs`
- `core/src/sqlite/sidebar.rs`
- compiler event emission call sites across domain write modules

## Implementation
1. Define an explicit derived-read-model publish boundary and manifest.
2. Separate domain writes from artifact publication responsibilities.
3. Reduce direct compiler event coupling from domain write paths where possible.
4. Make read-model artifact publication and epoch changes observable as backend runtime facts.

## Acceptance Criteria
1. Derived artifact publication is an explicit subsystem, not an implicit side effect chain.
2. Domain writes no longer need detailed compiler knowledge everywhere.
3. Publish completion can be surfaced cleanly to runtime synchronization.
4. Bitmap/sidebar/projection ownership boundaries are clearer.

## Test Cases
1. Domain writes still trigger the correct derived artifact rebuilds.
2. Sidebar and smart-folder projections remain correct.
3. Published artifact epochs remain consistent across rebuild cycles.

## Risk
High. This touches correctness-sensitive read-model behavior.
