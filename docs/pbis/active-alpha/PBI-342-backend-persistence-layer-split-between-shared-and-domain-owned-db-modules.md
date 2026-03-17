# PBI-342: Backend persistence layer split between shared and domain-owned DB modules

## Priority
P1

## Audit Status (2026-03-17)
Status: **Partially Implemented**

Evidence:
1. `core/src/sqlite/` still contains both shared DB infrastructure and domain-specific persistence logic.
2. `core/src/sqlite_ptr/` is already gone and is no longer part of this PBI.
3. Domain-owned persistence already exists for several areas:
   - `core/src/folders/db.rs`
   - `core/src/folders/collections_db.rs`
   - `core/src/subscriptions/db.rs`
   - `core/src/subscriptions/subscription_groups_db.rs`
   - `core/src/tags/db.rs`
4. The remaining ownership leaks are concentrated in:
   - `core/src/sqlite/projections.rs`
   - `core/src/sqlite/compilers.rs`
   - domain-leaking parts of `core/src/sqlite/files.rs`

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Problem
A contributor should not have to jump between a domain and `sqlite/*` just to understand one persistence flow. Shared SQLite infrastructure should stay centralized, but domain-specific queries and rebuild helpers should live with the domain that owns them.

## Scope
- `core/src/sqlite/projections.rs`
- `core/src/sqlite/compilers.rs`
- domain-leaking parts of `core/src/sqlite/files.rs`
- owner-correct persistence modules under:
  - `core/src/metadata/`
  - `core/src/sidebar/`
  - `core/src/smart_folders/`
  - `core/src/tags/`
  - `core/src/folders/`

## Implementation
1. Keep `core/src/sqlite/` as the shared SQLite infrastructure root for now.
2. Move metadata projection read/repair logic out of `core/src/sqlite/projections.rs` into metadata-owned persistence.
3. Split `core/src/sqlite/compilers.rs` so it only owns compiler batching/orchestration and publish handoff.
4. Move domain rebuild SQL into the owning domains:
   - tags
   - smart folders
   - sidebar
   - metadata projection rebuild helpers
5. Move folder/collection semantics out of `core/src/sqlite/files.rs` into folders-owned persistence.
6. Keep low-level file table CRUD shared in `core/src/sqlite/files.rs` until a dedicated media/file owner exists.

## Acceptance Criteria
1. Domain-local persistence is physically close to domain logic.
2. Shared SQLite infrastructure remains centralized without becoming a second home for domain behavior.
3. Metadata projection logic is metadata-owned.
4. Compiler orchestration is shared, but rebuild bodies are domain-owned.
5. `core/src/sqlite/files.rs` no longer owns folder/collection semantics.

## Test Cases
1. Build/tests pass.
2. Metadata batch reads and corrupt projection fallback still work.
3. Representative sidebar, smart-folder, tag, and folder/collection flows still work.

## Risk
High. Moves correctness-sensitive SQL helpers and must be staged carefully.
