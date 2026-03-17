# PBI-342: Backend persistence layer split between shared and domain-owned DB modules

## Priority
P1

## Completion Status (2026-03-17)
Status: **Implemented**

Completed outcome:
1. Metadata projection read/repair logic moved out of `core/src/sqlite/projections.rs` into metadata-owned persistence:
   - `core/src/metadata/db.rs`
2. `core/src/sqlite/compilers.rs` now owns compiler batching/orchestration only, while rebuild bodies live under their owning domains:
   - `core/src/tags/compiler.rs`
   - `core/src/smart_folders/compiler.rs`
   - `core/src/sidebar/compiler.rs`
   - `core/src/metadata/compiler.rs`
3. Folder/collection semantics were pulled out of `core/src/sqlite/files.rs` into folders-owned persistence:
   - `core/src/folders/db.rs`
   - `core/src/folders/collections_db.rs`
4. `core/src/sqlite/` remains the shared SQLite infrastructure root. This PBI intentionally did not rename it.

## Notes
1. Low-level file table CRUD remains in `core/src/sqlite/files.rs` by design.
2. The remaining media/file ownership question is intentionally deferred to the later media-lifecycle rewrite rather than inventing a fake owner in this PBI.

## Acceptance Criteria
1. Domain-local persistence is physically close to domain logic.
2. Shared SQLite infrastructure remains centralized without becoming a second home for domain behavior.
3. Metadata projection logic is metadata-owned.
4. Compiler orchestration is shared, but rebuild bodies are domain-owned.
5. `core/src/sqlite/files.rs` no longer owns folder/collection semantics.

## Validation
1. `cargo check --manifest-path ./core/Cargo.toml -q`
2. `cargo test --manifest-path ./core/Cargo.toml -q projection_corruption_is_tracked`
3. `cargo test --manifest-path ./core/Cargo.toml -q compiler_plan_accumulates_events`
4. `cargo test --manifest-path ./core/Cargo.toml -q sidebar_untagged_count_uses_active_minus_tagged`
5. `cargo test --manifest-path ./core/Cargo.toml -q smart_folder_bitmap_matches_sql`
6. `cargo test --manifest-path ./core/Cargo.toml -q collection_crud_roundtrip`
7. `cargo test --manifest-path ./core/Cargo.toml -q add_collection_members_by_hashes_roundtrip`
8. `cargo test --manifest-path ./core/Cargo.toml -q grid_page_slim_collection_scope_returns_only_collection_members`
