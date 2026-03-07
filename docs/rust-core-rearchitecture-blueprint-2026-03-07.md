# Rust Core Re-Architecture Blueprint

Date: 2026-03-07
Status: architecture blueprint, not implementation
Scope: `core/src/**`

## Purpose

This document is the actual backend re-architecture blueprint.

The earlier audit and PBI pass identified major problems, but this document is
what turns that into a real structural plan. It answers these questions:

1. What should the final `core/src` tree look like?
2. Where should every current backend file end up?
3. Which files should be merged?
4. Which files should be deleted?
5. Which files should stay where they are?
6. In what order should the migration happen so the codebase does not collapse
   under one giant rename PR?

This is written against the current actual tree, including the presence of:

- `core/src/runtime_contract/`
- `core/src/runtime_state.rs`

That means this blueprint supersedes any older assumption that the root is
entirely flat.

## The Real Structural Problem

The backend is not just "messy". It has three distinct structural problems at
once.

### 1. Top-level ownership is unclear

`core/src` still mixes:

- app lifecycle
- runtime synchronization
- infrastructure
- domain logic
- persistence
- media processing
- transport routing

in one top-level space.

### 2. Domain code is split across multiple homes

A single domain often lives in three places at once:

- root-level `*_controller.rs` / helpers
- `sqlite/*` or `sqlite_ptr/*`
- `dispatch/*`

That makes it hard to see what a domain actually owns.

### 3. Names do not reflect responsibilities

The codebase overuses names like:

- `controller`
- `files`
- `types`
- `sqlite`

for modules that actually own orchestration, services, policies, pipelines, or
read-model logic.

The result is that even when code works, it is difficult to reason about where a
change belongs.

## Final Target Tree

This is the target backend shape.

```text
core/src/
  app/
    mod.rs
    library_runtime.rs
    worker_runtime.rs
    startup.rs

  runtime/
    mod.rs
    events.rs
    receipts.rs
    task_registry.rs
    snapshot.rs
    sequence.rs

  infra/
    mod.rs
    blob_store.rs
    constants.rs
    credential_store.rs
    media_protocol.rs
    perf.rs
    poison.rs
    rate_limiter.rs

  media_processing/
    mod.rs
    detect.rs
    inspect.rs
    hash.rs
    thumbnails.rs
    adapters/
      mod.rs
      archive.rs
      ffmpeg.rs
      image.rs
      office.rs
      pdf.rs
      specialty.rs
      svg.rs
      blurhash.rs
      colors.rs
      gallery_dl_path.rs
      ffmpeg_path.rs

  persistence/
    mod.rs
    connection.rs
    schema/
      mod.rs
      core.rs
      entities.rs
      folders.rs
      tags.rs
      subscriptions.rs
      duplicates.rs
      projections.rs
    publish/
      mod.rs
      bitmaps.rs
      compilers.rs
      projections.rs
      sidebar.rs
    ptr/
      mod.rs
      connection.rs
      bootstrap.rs
      sync.rs
      overlay.rs
      cache.rs
      tags.rs

  domains/
    files/
      mod.rs
      controller.rs
      lifecycle.rs
      metadata.rs
      ingest.rs
      db.rs

    tags/
      mod.rs
      controller.rs
      normalize.rs
      relations.rs
      db.rs

    folders/
      mod.rs
      controller.rs
      membership.rs
      auto_tags.rs
      db.rs

    smart_folders/
      mod.rs
      controller.rs
      predicates.rs
      db.rs

    subscriptions/
      mod.rs
      controller.rs
      orchestrator.rs
      query_engine.rs
      progress.rs
      site_registry.rs
      gallery_dl/
        mod.rs
        runner.rs
        process.rs
        parser.rs
        failures.rs
        auth.rs
      db.rs

    flows/
      mod.rs
      controller.rs
      orchestrator.rs
      db.rs

    duplicates/
      mod.rs
      controller.rs
      matching.rs
      decisions.rs
      db.rs

    selection/
      mod.rs
      controller.rs
      query.rs
      summary.rs

    grid/
      mod.rs
      controller.rs
      query.rs
      scopes.rs
      cursors.rs

    ptr/
      mod.rs
      controller.rs
      client.rs
      sync.rs
      bootstrap.rs
      overlay.rs
      cache.rs
      tags.rs
      types.rs
      db.rs

    settings/
      mod.rs
      controller.rs
      view_prefs.rs
      store.rs
      db.rs

  dispatch/
    mod.rs
    common.rs
    files.rs
    files_lifecycle.rs
    files_media.rs
    files_metadata.rs
    folders.rs
    grid.rs
    ptr.rs
    selection.rs
    smart_folders.rs
    subscriptions.rs
    system.rs
    tags.rs

  lib.rs
```

## What Stays Top-Level

Only these should remain top-level in the final state:

- `lib.rs`
- the directories above

Everything else should be moved under one of those directories.

## File-by-File Mapping

This is the important part.

## Current root files

### Move to `app/`

- `core/src/state.rs` -> `core/src/app/library_runtime.rs`

Notes:
- split worker boot/shutdown into `worker_runtime.rs`
- if startup helper extraction is needed, introduce `startup.rs`

### Move to `runtime/`

- `core/src/events.rs` -> `core/src/runtime/events.rs`
- `core/src/runtime_state.rs` -> `core/src/runtime/task_registry.rs`
- `core/src/runtime_contract/mod.rs` -> `core/src/runtime/mod.rs` or `core/src/runtime_contract/` folded under `runtime/`
- `core/src/runtime_contract/mutation.rs` -> `core/src/runtime/receipts.rs`
- `core/src/runtime_contract/task.rs` -> `core/src/runtime/task_registry.rs` or `core/src/runtime/task_types.rs`
- `core/src/runtime_contract/snapshot.rs` -> `core/src/runtime/snapshot.rs`

Decision:
- there should not be both `runtime/` and `runtime_contract/` long-term
- fold `runtime_contract/*` into `runtime/*`

### Move to `infra/`

- `core/src/blob_store.rs` -> `core/src/infra/blob_store.rs`
- `core/src/constants.rs` -> `core/src/infra/constants.rs`
- `core/src/credential_store.rs` -> `core/src/infra/credential_store.rs`
- `core/src/media_protocol.rs` -> `core/src/infra/media_protocol.rs`
- `core/src/perf.rs` -> `core/src/infra/perf.rs`
- `core/src/poison.rs` -> `core/src/infra/poison.rs`
- `core/src/rate_limiter.rs` -> `core/src/infra/rate_limiter.rs`

### Move to `media_processing/`

- `core/src/files/mod.rs` -> `core/src/media_processing/mod.rs`
- `core/src/files/archive.rs` -> `core/src/media_processing/adapters/archive.rs`
- `core/src/files/blurhash.rs` -> `core/src/media_processing/adapters/blurhash.rs`
- `core/src/files/colors.rs` -> `core/src/media_processing/adapters/colors.rs`
- `core/src/files/ffmpeg.rs` -> `core/src/media_processing/adapters/ffmpeg.rs`
- `core/src/files/ffmpeg_path.rs` -> `core/src/media_processing/adapters/ffmpeg_path.rs`
- `core/src/files/gallery_dl_path.rs` -> `core/src/media_processing/adapters/gallery_dl_path.rs`
- `core/src/files/image_metadata.rs` -> `core/src/media_processing/adapters/image.rs`
- `core/src/files/office.rs` -> `core/src/media_processing/adapters/office.rs`
- `core/src/files/pdf.rs` -> `core/src/media_processing/adapters/pdf.rs`
- `core/src/files/specialty.rs` -> `core/src/media_processing/adapters/specialty.rs`
- `core/src/files/svg.rs` -> `core/src/media_processing/adapters/svg.rs`

Decision:
- `PBI-237` is only the first rename step
- the final shape should be adapter- and capability-oriented, not one giant `mod.rs`

### Move to `domains/files/`

- `core/src/import.rs` -> `core/src/domains/files/ingest.rs`
- `core/src/import_controller.rs` -> `core/src/domains/files/controller.rs` or delete if folded into controller facade
- `core/src/lifecycle_controller.rs` -> `core/src/domains/files/lifecycle.rs`
- `core/src/metadata_controller.rs` -> `core/src/domains/files/metadata.rs`

Decision:
- `metadata_controller.rs` should not become its own top-level `metadata` domain unless it remains large after consolidation
- default plan is to fold metadata into `domains/files/`

### Move to `domains/tags/`

- `core/src/tag_controller.rs` -> `core/src/domains/tags/controller.rs`
- `core/src/tags.rs` -> `core/src/domains/tags/normalize.rs` plus `relations.rs` if split is needed

### Move to `domains/folders/`

- `core/src/folder_controller.rs` -> `core/src/domains/folders/controller.rs`

### Move to `domains/smart_folders/`

- `core/src/smart_folder_controller.rs` -> `core/src/domains/smart_folders/controller.rs`

### Move to `domains/subscriptions/`

- `core/src/subscription_controller.rs` -> `core/src/domains/subscriptions/controller.rs`
- `core/src/subscription_sync.rs` -> `core/src/domains/subscriptions/query_engine.rs`
- `core/src/gallery_dl_runner.rs` -> `core/src/domains/subscriptions/gallery_dl/runner.rs`

Notes:
- runner decomposition later splits this further
- this domain should also own progress/orchestration helpers

### Move to `domains/flows/`

- `core/src/flow_controller.rs` -> `core/src/domains/flows/controller.rs`

### Move to `domains/duplicates/`

- `core/src/duplicate_controller.rs` -> `core/src/domains/duplicates/controller.rs`
- `core/src/duplicates.rs` -> `core/src/domains/duplicates/matching.rs`

### Move to `domains/selection/`

- `core/src/selection_controller.rs` -> `core/src/domains/selection/controller.rs`
- `core/src/selection_helpers.rs` -> `core/src/domains/selection/query.rs` and/or `summary.rs`

### Move to `domains/grid/`

- `core/src/grid_controller.rs` -> `core/src/domains/grid/controller.rs`

### Move to `domains/ptr/`

- `core/src/ptr_client.rs` -> `core/src/domains/ptr/client.rs`
- `core/src/ptr_controller.rs` -> `core/src/domains/ptr/controller.rs`
- `core/src/ptr_sync.rs` -> `core/src/domains/ptr/sync.rs`
- `core/src/ptr_types.rs` -> `core/src/domains/ptr/types.rs`

### Move to `domains/settings/`

- `core/src/settings.rs` -> `core/src/domains/settings/store.rs`
- `core/src/view_prefs_controller.rs` -> `core/src/domains/settings/view_prefs.rs`

### Delete or absorb

- `core/src/sidebar_controller.rs`

Decision:
- sidebar should not remain a standalone top-level domain
- its read-side logic should be absorbed by `domains/grid/` + `domains/folders/` + `persistence/publish/sidebar.rs`
- if a controller facade is still needed, place it under `domains/grid/` or `domains/folders/`, but do not keep a standalone root-level sidebar controller

- `core/src/types.rs`

Decision:
- split over time
- short term: move to `infra/types.rs`
- long term: migrate domain-specific structs out of it

## Current `dispatch/*`

### Keep under `dispatch/`

- `core/src/dispatch/mod.rs`
- `core/src/dispatch/common.rs`
- `core/src/dispatch/files.rs`
- `core/src/dispatch/files_lifecycle.rs`
- `core/src/dispatch/files_media.rs`
- `core/src/dispatch/files_metadata.rs`
- `core/src/dispatch/folders.rs`
- `core/src/dispatch/grid.rs`
- `core/src/dispatch/ptr.rs`
- `core/src/dispatch/selection.rs`
- `core/src/dispatch/smart_folders.rs`
- `core/src/dispatch/subscriptions.rs`
- `core/src/dispatch/system.rs`
- `core/src/dispatch/tags.rs`

### Delete/merge from `dispatch/`

- `core/src/dispatch/files_review.rs`

Decision:
- this should be deleted under `PBI-236`
- review is file lifecycle/media, not its own dispatch domain

- `core/src/dispatch/duplicates.rs`

Decision:
- keep only if duplicates remains a transport-facing first-class domain
- otherwise fold into `files`/`selection`/`system` command routing later
- not immediate, but should be revisited after domain cleanup

## Current `sqlite/*`

This is where the old architecture is still most misleading.

### Must remain shared infrastructure in `persistence/`

- `core/src/sqlite/mod.rs` -> `core/src/persistence/connection.rs` + `mod.rs`
- `core/src/sqlite/schema.rs` -> `core/src/persistence/schema/*`
- `core/src/sqlite/bitmaps.rs` -> `core/src/persistence/publish/bitmaps.rs`
- `core/src/sqlite/compilers.rs` -> `core/src/persistence/publish/compilers.rs`
- `core/src/sqlite/projections.rs` -> `core/src/persistence/publish/projections.rs`
- `core/src/sqlite/sidebar.rs` -> `core/src/persistence/publish/sidebar.rs`
- `core/src/sqlite/hash_index.rs` -> `core/src/persistence/publish/hash_index.rs` or `connection.rs` if truly shared

### Must move under domains as domain-owned DB modules

- `core/src/sqlite/files.rs` -> `core/src/domains/files/db.rs`
- `core/src/sqlite/import.rs` -> `core/src/domains/files/db_import.rs` or fold into `db.rs`
- `core/src/sqlite/tags.rs` -> `core/src/domains/tags/db.rs`
- `core/src/sqlite/folders.rs` -> `core/src/domains/folders/db.rs`
- `core/src/sqlite/smart_folders.rs` -> `core/src/domains/smart_folders/db.rs`
- `core/src/sqlite/subscriptions.rs` -> `core/src/domains/subscriptions/db.rs`
- `core/src/sqlite/flows.rs` -> `core/src/domains/flows/db.rs`
- `core/src/sqlite/duplicates.rs` -> `core/src/domains/duplicates/db.rs`
- `core/src/sqlite/collections.rs` -> `core/src/domains/files/collections_db.rs` or `domains/files/db_collections.rs`
- `core/src/sqlite/view_prefs.rs` -> `core/src/domains/settings/db.rs`

## Current `sqlite_ptr/*`

### Short-term

Keep as a grouped subsystem while PTR is still large.

### Long-term target

Move under:

- `core/src/persistence/ptr/*` for shared PTR storage infrastructure
- `core/src/domains/ptr/db.rs` and `core/src/domains/ptr/*` for domain-facing orchestration/use

Concrete mapping:

- `core/src/sqlite_ptr/mod.rs` -> `core/src/persistence/ptr/mod.rs`
- `core/src/sqlite_ptr/bootstrap.rs` -> `core/src/persistence/ptr/bootstrap.rs`
- `core/src/sqlite_ptr/cache.rs` -> `core/src/persistence/ptr/cache.rs`
- `core/src/sqlite_ptr/overlay.rs` -> `core/src/persistence/ptr/overlay.rs`
- `core/src/sqlite_ptr/sync.rs` -> `core/src/persistence/ptr/sync.rs`
- `core/src/sqlite_ptr/tags.rs` -> `core/src/persistence/ptr/tags.rs`

Decision:
- keep a domain facade in `domains/ptr/`
- do not expose `persistence/ptr/*` directly to the whole crate long-term

## `lib.rs` End State

Today `lib.rs` is effectively a flat export list.

Final shape should look more like:

```rust
pub mod app;
pub mod runtime;
pub mod infra;
pub mod media_processing;
pub mod persistence;
pub mod domains;
pub mod dispatch;
```

Optional temporary compatibility exports are acceptable during migration, but
must be deleted under the final cleanup phase.

## Merge/Delete Decisions

These are the important explicit calls.

### Merge

1. `runtime_contract/*` into `runtime/*`
2. `metadata_controller.rs` into `domains/files/`
3. `import_controller.rs` into `domains/files/controller.rs` or delete if redundant
4. `sqlite/import.rs` into `domains/files/db.rs` or `db_import.rs`
5. `sqlite/collections.rs` into files/entity domain persistence
6. selection/sidebar shared read logic into proper domain query services

### Delete

1. `dispatch/files_review.rs`
2. legacy root-level re-export aliases after migration
3. standalone `sidebar_controller.rs` as a first-class root module
4. any temporary old-path module kept only for compatibility after imports are migrated

### Keep but move

1. `dispatch/*` remains transport routing
2. `files/*` becomes `media_processing/*`
3. `sqlite_ptr/*` stays grouped initially but moves under `persistence/ptr/*`

## Migration Phases

## Phase 0: Freeze new root-level sprawl

Rule:
- no new root-level backend files unless they belong to `app`, `runtime`,
  `infra`, `media_processing`, `persistence`, `domains`, or `dispatch`

## Phase 1: Top-level skeleton

Create:

- `app/`
- `runtime/`
- `infra/`
- `media_processing/`
- `persistence/`
- `domains/`

Do not move all logic yet.

## Phase 2: Move obvious shared infrastructure

Move:

- `blob_store.rs`
- `constants.rs`
- `credential_store.rs`
- `media_protocol.rs`
- `perf.rs`
- `poison.rs`
- `rate_limiter.rs`
- `events.rs`
- `state.rs`

This immediately reduces root noise.

## Phase 3: Rename `files/` to `media_processing/`

This is already recognized by `PBI-237`.

Do this early because the name collision with domain `files` makes the rest of
folderization harder.

## Phase 4: Domain folderization in clusters

Cluster A:
- tags
- folders
- smart_folders

Cluster B:
- selection
- grid
- duplicates

Cluster C:
- files/import/lifecycle/metadata

Cluster D:
- subscriptions
- flows

Cluster E:
- ptr

Cluster F:
- settings

## Phase 5: Persistence split

Move shared infrastructure into `persistence/`.
Move domain-specific persistence into `domains/*/db.rs`.

## Phase 6: Controller naming normalization

After files are in the right folders, rename modules based on actual
responsibility.

## Phase 7: Alias deletion

Delete old root paths and any compatibility exports.

## What This Blueprint Changes About Existing PBIs

### `PBI-233`

Should be treated as the umbrella goal, not the executable plan.

### `PBI-310` to `PBI-314`

These are the executable structural PBIs for physical backend cleanup.

### `PBI-300` to `PBI-309`

These are service/runtime/persistence architecture PBIs that sit inside the new
physical tree.

In short:

- `PBI-310..314` = where code lives
- `PBI-300..309` = how major backend subsystems are internally shaped

Both are required.

## Definition of Done

The backend is structurally re-architected when all of the following are true:

1. `core/src` root is mostly directories plus `lib.rs`
2. every domain lives in one domain folder
3. shared infrastructure is not mixed with domain code
4. shared persistence is not mixed with domain persistence
5. runtime/app/infra are physically distinct
6. old root-level aliases are gone
7. a contributor can locate a responsibility by folder, not by tribal memory

## Final Assessment

No, the backend was not "done" before this blueprint.

The earlier audit identified many important problems, but this blueprint is the
first document in this pass that actually answers the physical architecture
question end to end.
