# Rust Core Target Module Tree

Date: 2026-03-07
Purpose: define the physical `core/src` layout the backend should move toward.

## Problem

`core/src` is currently a flat root with controllers, domain logic, runtime
state, orchestration, and infrastructure all mixed together.

That makes it hard to answer basic questions:

- what is a domain module?
- what is shared infrastructure?
- what is runtime/process orchestration?
- what is persistence?
- what is media processing?
- what is still legacy and can be deleted?

The target structure below is about physical ownership, not just conceptual
boundaries.

## Target Top-Level Layout

```text
core/src/
  app/                 # application runtime, library lifecycle, worker startup
  runtime/             # runtime events, task registry, mutation receipts
  infra/               # cross-cutting utilities with no domain ownership
  media_processing/    # file/media inspection, hashing, rendering, transforms
  persistence/         # shared DB infrastructure only
  domains/
    files/
    tags/
    folders/
    smart_folders/
    subscriptions/
    flows/
    duplicates/
    selection/
    grid/
    ptr/
    settings/
    metadata/
  dispatch/            # transport-facing routing layer only
  lib.rs
```

## Ownership Rules

### `app/`

Owns:

- library open/close
- runtime/service construction
- worker startup/shutdown
- scheduler/process lifecycle wiring

Must not own:

- domain-specific logic
- renderer-facing invalidation semantics
- SQL query logic

Current candidates:

- `state.rs`

### `runtime/`

Owns:

- runtime event bus
- task registry
- mutation receipts
- runtime snapshots
- sequence numbers

Must not own:

- UI-specific concepts like sidebar/grid invalidation names
- domain write logic

Current candidates:

- `events.rs`
- future task/runtime state extracted from `subscription_sync.rs`, `ptr_controller.rs`, `flow_controller.rs`

### `infra/`

Owns:

- cross-cutting primitives and utilities
- no domain behavior

Current candidates:

- `blob_store.rs`
- `constants.rs`
- `credential_store.rs`
- `media_protocol.rs`
- `perf.rs`
- `poison.rs`
- `rate_limiter.rs`
- `types.rs` (or split later into domain-local types)

### `media_processing/`

Owns:

- MIME detection
- hashing
- inspection
- thumbnail/render generation
- format adapters

Current candidates:

- current `files/` directory after `PBI-237`

### `persistence/`

Owns only shared DB infrastructure:

- connection pool
- transaction helpers
- schema runner
- migration registry
- shared publish/manifest plumbing

Must not own domain-specific CRUD/query logic long-term.

Current candidates:

- pieces of `sqlite/mod.rs`
- pieces of `sqlite/schema.rs`
- pieces of `sqlite/compilers.rs`
- pieces of `sqlite/bitmaps.rs`
- pieces of `sqlite/projections.rs`

### `domains/*`

Each domain owns:

- controller/service/orchestration for that domain
- domain-local types
- domain-local persistence adapters
- domain-local query helpers
- domain-local sync/import helpers

Each domain should be navigable in one folder.

## Target Domain Mapping

### `domains/files/`

Target contents:

```text
domains/files/
  controller.rs
  lifecycle.rs
  metadata.rs
  import_pipeline.rs
  db.rs
  mod.rs
```

Current sources:

- `import.rs`
- `import_controller.rs`
- `lifecycle_controller.rs`
- `metadata_controller.rs`
- relevant file lifecycle logic from `dispatch/files_*`
- relevant DB logic from `sqlite/files.rs` and `sqlite/import.rs`

### `domains/tags/`

Target contents:

```text
domains/tags/
  controller.rs
  normalize.rs
  relations.rs
  db.rs
  mod.rs
```

Current sources:

- `tag_controller.rs`
- `tags.rs`
- `sqlite/tags.rs`

### `domains/folders/`

Target contents:

```text
domains/folders/
  controller.rs
  membership.rs
  auto_tags.rs
  db.rs
  mod.rs
```

Current sources:

- `folder_controller.rs`
- `sqlite/folders.rs`
- folder-related helpers in other controllers

### `domains/smart_folders/`

Target contents:

```text
domains/smart_folders/
  controller.rs
  predicate.rs
  db.rs
  mod.rs
```

Current sources:

- `smart_folder_controller.rs`
- `sqlite/smart_folders.rs`

### `domains/subscriptions/`

Target contents:

```text
domains/subscriptions/
  controller.rs
  orchestrator.rs
  query_engine.rs
  progress.rs
  site_registry.rs
  gallery_dl/
    runner.rs
    process.rs
    parser.rs
    failures.rs
    auth.rs
  db.rs
  mod.rs
```

Current sources:

- `subscription_controller.rs`
- `subscription_sync.rs`
- `gallery_dl_runner.rs`
- `sqlite/subscriptions.rs`

### `domains/flows/`

Target contents:

```text
domains/flows/
  controller.rs
  orchestrator.rs
  db.rs
  mod.rs
```

Current sources:

- `flow_controller.rs`
- `sqlite/flows.rs`

### `domains/duplicates/`

Target contents:

```text
domains/duplicates/
  controller.rs
  matching.rs
  decisions.rs
  db.rs
  mod.rs
```

Current sources:

- `duplicate_controller.rs`
- `duplicates.rs`
- `sqlite/duplicates.rs`

### `domains/selection/`

Target contents:

```text
domains/selection/
  controller.rs
  summary.rs
  query.rs
  mod.rs
```

Current sources:

- `selection_controller.rs`
- `selection_helpers.rs`

### `domains/grid/`

Target contents:

```text
domains/grid/
  controller.rs
  query.rs
  scopes.rs
  cursors.rs
  mod.rs
```

Current sources:

- `grid_controller.rs`
- scope/query logic currently spread into selection/sidebar helpers

### `domains/ptr/`

Target contents:

```text
domains/ptr/
  controller.rs
  client.rs
  sync.rs
  bootstrap.rs
  overlay.rs
  cache.rs
  tags.rs
  db.rs
  types.rs
  mod.rs
```

Current sources:

- `ptr_controller.rs`
- `ptr_client.rs`
- `ptr_sync.rs`
- `ptr_types.rs`
- `sqlite_ptr/*`

### `domains/settings/`

Target contents:

```text
domains/settings/
  controller.rs
  view_prefs.rs
  store.rs
  db.rs
  mod.rs
```

Current sources:

- `settings.rs`
- `view_prefs_controller.rs`
- `sqlite/view_prefs.rs`

### `domains/metadata/`

This may remain folded into `files/` if it stays small.
If not, make it a real domain.

## Dispatch Rule

`dispatch/` stays top-level.

It is the transport/routing layer only.
It should import domain modules, not own business logic.

Allowed in `dispatch/`:

- request decoding
- domain routing
- compatibility aliases

Not allowed in `dispatch/` long-term:

- meaningful lifecycle logic
- query orchestration
- duplicated invalidation semantics

## Deletion Rule

After the migration:

- there should be no domain/controller files left directly under `core/src/`
- top-level root should be mostly directories plus `lib.rs`
- legacy aliases should be deleted once all imports are updated

## Migration Strategy

Do not move everything at once.

Order:

1. create target directories and `mod.rs` shells
2. move one domain at a time
3. update imports and tests for that domain
4. only then delete old root-level files
5. keep temporary re-export aliases in `lib.rs` only when necessary
6. remove re-exports at the end

## Definition of Done

The backend is physically organized when:

1. `core/src` root is mostly directories, not domain files
2. each domain is discoverable in one folder
3. persistence is split between shared infrastructure and domain-local adapters
4. runtime/app/infra are distinct top-level folders
5. a contributor can navigate by responsibility rather than by filename memory
