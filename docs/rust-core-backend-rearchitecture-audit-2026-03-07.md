# Rust Core Backend Re-Architecture Audit

Date: 2026-03-07
Related umbrella: `PBI-240`
Related architecture docs:

- `docs/backend-frontend-state-rearchitecture.md`
- `docs/pbi-234-runtime-communication-implementation-plan.md`

## Purpose

This audit pass catalogs the major backend re-architecture tracks still missing
from the Rust core and turns them into implementation-ready PBIs.

This is an architecture-focused audit, not a line-by-line bug scrub. The goal is
not to enumerate every code smell. The goal is to identify the large structural
problems that are currently driving instability, state drift, and contributor
confusion.

## Review Method

This pass included:

1. full inventory of `core/src/`, `core/src/sqlite/`, `core/src/sqlite_ptr/`,
   `core/src/files/`, and `core/src/dispatch/`
2. overlap review against existing backend cleanup PBIs `233` through `239`
3. hotspot analysis by file size and responsibility concentration
4. targeted read-through of the largest and most architecturally important
   modules:
   - `core/src/events.rs`
   - `core/src/state.rs`
   - `core/src/subscription_controller.rs`
   - `core/src/subscription_sync.rs`
   - `core/src/gallery_dl_runner.rs`
   - `core/src/sqlite/schema.rs`
   - `core/src/files/mod.rs`
   - `core/src/ptr_controller.rs`

## Existing PBIs Already Covering Backend Architecture

These should remain as the existing backbone of the cleanup plan:

1. `PBI-233` Rust core domain folder realignment
2. `PBI-234` typed dispatch contract between core and frontend
3. `PBI-235` deduplicate `MutationImpact` construction
4. `PBI-236` merge `files_review` into `files_lifecycle`
5. `PBI-237` rename `files/` module to `media_processing/`
6. `PBI-238` unify tag parsing paths
7. `PBI-239` core module documentation and contract comments

## Main Findings

### 1. Runtime communication and task state are fragmented

The backend currently has several overlapping runtime state mechanisms:

- `events.rs` owns broad mutation invalidation events
- `subscription_sync.rs` owns a dedicated in-memory subscription progress map
- `ptr_controller.rs` owns global sync/bootstrap flags and progress storage
- `flow_controller.rs` emits its own runtime progress events
- `state.rs` wires compiler completion directly back into UI-facing events

This is the biggest structural cause of frontend/backend drift.

This is covered by:

- `PBI-234`
- new `PBI-300`

### 2. `state.rs` is a service locator plus process orchestrator

`core/src/state.rs` owns:

- global singleton state
- library open/close
- worker spawning
- scheduler startup
- compiler loop wiring
- PTR path policy
- lifecycle cancellation

That is too much responsibility for one module and it makes library switching,
worker cleanup, and future testing unnecessarily risky.

This is covered by new `PBI-301`.

### 3. Subscription architecture is split along historical seams, not clean boundaries

Current split:

- `subscription_controller.rs` mixes CRUD, orchestration, run/stop/reset logic,
  archive reset, progress shaping, and query naming rules
- `subscription_sync.rs` mixes sync orchestration, dedupe behavior, metadata
  merge semantics, runtime progress, collection grouping, and resume logic
- `gallery_dl_runner.rs` mixes site registry, auth shaping, temp config
  generation, process spawning, output scanning, metadata parsing, and failure
  interpretation

This needs to become a layered subscription domain rather than three giant
hybrid files.

This is covered by new `PBI-302` and `PBI-303`.

### 4. SQLite schema and derived read models are too centralized

`core/src/sqlite/schema.rs` is 1700+ lines and contains both schema definition
and historical migration logic for many unrelated domains.

`sqlite/compilers.rs`, `sqlite/bitmaps.rs`, and `sqlite/projections.rs` also
represent a cross-cutting derived-read-model system that is powerful but too
implicit. Write paths and publish paths are not cleanly separated.

This is covered by new `PBI-304` and `PBI-305`.

### 5. Import/entity lifecycle is spread across multiple weakly-defined modules

Relevant modules:

- `import.rs`
- `import_controller.rs`
- `lifecycle_controller.rs`
- `metadata_controller.rs`
- `sqlite/import.rs`
- parts of `subscription_sync.rs`
- parts of `duplicate_controller.rs`

The entity model, import pipeline, metadata preservation, duplicate merge, and
collection grouping rules are not owned by one coherent subsystem.

This is covered by new `PBI-306`.

### 6. Read-side query concerns are scattered across grid, selection, sidebar, and compilers

Relevant hotspots:

- `grid_controller.rs` (1173 lines)
- `selection_helpers.rs` (439 lines)
- `selection_controller.rs`
- `sidebar_controller.rs`
- `sqlite/sidebar.rs`
- `sqlite/projections.rs`

The result is that read-model ownership is not obvious and invalidation is hard
to reason about.

This is covered by new `PBI-307`.

### 7. PTR is its own mini-application inside the backend

Relevant modules:

- `ptr_controller.rs`
- `ptr_sync.rs`
- `ptr_client.rs`
- `ptr_types.rs`
- `sqlite_ptr/bootstrap.rs`
- `sqlite_ptr/sync.rs`
- `sqlite_ptr/overlay.rs`
- `sqlite_ptr/tags.rs`
- `sqlite_ptr/cache.rs`

PTR currently contains controller state, task state, bootstrap, delta sync,
compaction, overlay handling, and cache behavior across several very large
files. The domain boundaries are better than some other areas, but still too
controller-centric and too global-state-heavy.

This is covered by new `PBI-308`.

### 8. Media processing has grown into a format platform without a clear adapter boundary

`core/src/files/mod.rs` is 1000+ lines and the surrounding format-specific
modules (`office.rs`, `pdf.rs`, `specialty.rs`, `ffmpeg.rs`, `svg.rs`) are
organized as helpers rather than a clear adapter pipeline.

This area needs a cleaner capability registry and format adapter model before it
keeps growing.

This is covered by new `PBI-309`.

## Hotspot Evidence

Largest modules at the time of audit:

1. `core/src/gallery_dl_runner.rs` — 2445 lines
2. `core/src/sqlite/schema.rs` — 1728 lines
3. `core/src/sqlite_ptr/bootstrap.rs` — 1713 lines
4. `core/src/sqlite/files.rs` — 1632 lines
5. `core/src/subscription_sync.rs` — 1576 lines
6. `core/src/sqlite/collections.rs` — 1339 lines
7. `core/src/sqlite/compilers.rs` — 1266 lines
8. `core/src/sqlite/tags.rs` — 1242 lines
9. `core/src/ptr_sync.rs` — 1210 lines
10. `core/src/grid_controller.rs` — 1173 lines
11. `core/src/subscription_controller.rs` — 1075 lines
12. `core/src/files/mod.rs` — 1054 lines
13. `core/src/sqlite_ptr/sync.rs` — 1049 lines
14. `core/src/ptr_controller.rs` — 1016 lines
15. `core/src/sqlite/folders.rs` — 976 lines

These are not automatically wrong, but in this codebase they correspond closely
with mixed responsibilities and unclear ownership boundaries.

## New PBIs Created In This Audit

1. `PBI-300` Rust core runtime event bus and task registry realignment
2. `PBI-301` app state service lifecycle and worker boundary cleanup
3. `PBI-302` subscription domain service split and orchestration cleanup
4. `PBI-303` gallery-dl runner decomposition and site adapter split
5. `PBI-304` SQLite schema and migration pack decomposition
6. `PBI-305` derived read-model publish boundary cleanup for SQLite
7. `PBI-306` import, lifecycle, and entity pipeline realignment
8. `PBI-307` grid, selection, and sidebar query service decomposition
9. `PBI-308` PTR domain decomposition and runtime state cleanup
10. `PBI-309` media processing adapter registry and pipeline breakup

## Recommended Execution Order

1. `PBI-234` typed/runtime communication foundation
2. `PBI-300` runtime event bus and task registry
3. `PBI-301` app state lifecycle cleanup
4. `PBI-302` subscription domain split
5. `PBI-303` gallery-dl runner decomposition
6. `PBI-304` schema/migration decomposition
7. `PBI-305` derived read-model publish boundaries
8. `PBI-306` import/entity lifecycle realignment
9. `PBI-307` grid/selection/sidebar query service split
10. `PBI-308` PTR domain decomposition
11. `PBI-309` media processing adapter registry
12. `PBI-233` physical module/folder realignment once the service boundaries are
    clearer
13. `PBI-239` documentation pass once the architecture settles

## What This Audit Does Not Claim

This report does not claim that every low-level bug has been catalogued.

It does claim that the major backend re-architecture tracks are now mapped and
split into concrete work items, which is what `PBI-240` needed to accomplish in
this pass.
