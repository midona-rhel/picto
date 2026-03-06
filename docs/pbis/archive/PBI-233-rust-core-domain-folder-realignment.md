# PBI-233: Rust core domain folder realignment

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `core/src/` has ~40 files at the top level with no domain grouping.
2. Only `dispatch/`, `files/`, `sqlite/`, and `sqlite_ptr/` are organized into subdirectories.
3. Controllers, domain types, sync logic, and infrastructure sit side-by-side with no separation.
4. Frontend has PBI-166 for the same problem; the Rust core has no equivalent.

## Problem
The Rust core's `src/` directory is a flat bag of files. Related modules (e.g. `ptr_client.rs`, `ptr_controller.rs`, `ptr_sync.rs`, `ptr_types.rs`) are not grouped, making it hard to understand domain boundaries or find related code. As the core grows, this will become increasingly painful for contributors.

## Current flat files and their natural domains

| Domain | Files |
|---|---|
| **duplicates** | `duplicate_controller.rs`, `duplicates.rs` |
| **folders** | `folder_controller.rs` |
| **tags** | `tag_controller.rs`, `tags.rs` |
| **ptr** | `ptr_client.rs`, `ptr_controller.rs`, `ptr_sync.rs`, `ptr_types.rs` |
| **subscriptions** | `subscription_controller.rs`, `subscription_sync.rs`, `gallery_dl_runner.rs` |
| **import** | `import.rs`, `import_controller.rs` |
| **selection** | `selection_controller.rs`, `selection_helpers.rs` |
| **grid** | `grid_controller.rs` |
| **sidebar** | `sidebar_controller.rs` |
| **smart_folders** | `smart_folder_controller.rs` |
| **flows** | `flow_controller.rs` |
| **settings** | `settings.rs`, `view_prefs_controller.rs` |
| **metadata** | `metadata_controller.rs` |
| **lifecycle** | `lifecycle_controller.rs` |
| **infra/shared** | `blob_store.rs`, `state.rs`, `types.rs`, `constants.rs`, `events.rs`, `perf.rs`, `poison.rs`, `rate_limiter.rs`, `media_protocol.rs`, `credential_store.rs` |

## Scope
- `core/src/` — reorganize flat controller/domain files into domain modules
- `core/src/sqlite/` — split the monolithic sqlite directory so each domain owns its persistence
- `core/src/dispatch/` — stays at the top level (it's the routing layer, splitting it per-domain breaks the central routing pattern)
- `core/src/lib.rs` — update module declarations
- `native/picto-node/src/lib.rs` — update any direct imports if affected

## Target structure

Each domain gets a directory containing its controller, types, persistence, and sync logic:

```
core/src/
  tags/
    controller.rs        (from tag_controller.rs)
    types.rs             (from tags.rs)
    db.rs                (from sqlite/tags.rs)
    mod.rs
  ptr/
    client.rs            (from ptr_client.rs)
    controller.rs        (from ptr_controller.rs)
    sync.rs              (from ptr_sync.rs)
    types.rs             (from ptr_types.rs)
    db.rs                (from sqlite_ptr/)
    mod.rs
  folders/
    controller.rs        (from folder_controller.rs)
    db.rs                (from sqlite/folders.rs)
    mod.rs
  ...etc
  sqlite/
    mod.rs               (shared DB infra: connection pool, migrations, schema)
    bitmaps.rs           (cross-cutting bitmap index)
    compilers.rs         (cross-cutting compiler pipeline)
    schema.rs
  dispatch/              (stays at top level — routing layer)
    mod.rs
    files.rs
    tags.rs
    ...
  infra/                 (cross-cutting utilities)
    blob_store.rs
    state.rs
    events.rs
    ...
```

## Implementation
1. Create domain directories under `core/src/` for each cluster (e.g. `ptr/`, `subscriptions/`, `duplicates/`, `tags/`, `folders/`, `import/`, `selection/`, `settings/`, `flows/`).
2. Move controllers, domain types, and sync logic into their domain directory.
3. Move domain-specific persistence from `sqlite/` into each domain directory (e.g. `sqlite/tags.rs` → `tags/db.rs`). Keep shared DB infrastructure (connection pool, schema, bitmaps, compilers) in `sqlite/`.
4. Group infrastructure/shared modules under `core/src/infra/` (or keep them at root if they are genuinely cross-cutting — e.g. `state.rs`, `types.rs`, `constants.rs`).
5. Leave `dispatch/` at the top level — it's the central routing layer and splitting it breaks the pattern.
6. Update `lib.rs` module declarations to point to the new paths.
7. Fix all internal `use` paths across the crate.
8. Verify `picto-node` bindings still compile and resolve correctly.
9. Do in phased batches (one domain at a time) to keep diffs reviewable.

## Acceptance Criteria
1. No flat controller/domain files remain directly in `core/src/` — each belongs to a domain module.
2. Each domain directory contains its controller, types, and persistence together.
3. `sqlite/` only contains shared DB infrastructure, not domain-specific queries.
4. `dispatch/` remains at the top level as the routing layer.
5. Shared infrastructure is clearly separated from domain logic.
6. `cargo build` and `cargo test` pass after each batch move.
7. `picto-node` native bindings compile and tests pass.
8. A new contributor can find all PTR-related code in one directory, all subscription code in one directory, etc.

## Test Cases
1. `cargo build` — compiles cleanly after full realignment.
2. `cargo test` — all existing tests pass.
3. `npm run build` (picto-node) — native addon builds successfully.
4. Application smoke test — all features work with no regressions.

## Risk
Medium-high. Large-scale file moves across a Rust crate require updating all internal `use` paths. Phased batches (one domain per PR) reduce risk. Similar scope to PBI-166 on the frontend side.
