# PBI-311: Derived read-model publish boundary cleanup for SQLite

## Priority
P1

## Status
Implemented

## Completed Work
1. Added an explicit read-model contract in `./core/src/sqlite/read_model.rs`.
2. Added a dedicated publish boundary in `./core/src/sqlite/publish.rs`.
3. Moved domain write paths onto `ReadModelEvent` emission instead of direct compiler coupling.
4. Updated `./core/src/sqlite/compilers.rs` to rebuild read models, then publish through the manifest boundary.
5. Surfaced publish completion as a runtime event from `./core/src/workers.rs`.

## Outcome
1. Derived artifact publication is an explicit subsystem, not an implicit side-effect chain.
2. Domain writes no longer need detailed compiler knowledge.
3. Publish completion is visible to runtime synchronization.
4. Bitmap, sidebar, projection, and manifest ownership boundaries are clearer.
