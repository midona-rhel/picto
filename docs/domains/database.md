# Database Domain

SQLite is the library source of truth. Derived bitmaps, sidebar rows, thumbnails, colors, and
duplicate candidates may be rebuilt from it.

## Ownership

- `core/src/db/core/schema.rs` owns the complete current schema and exact-version validation.
- `core/src/db/query/` owns read queries.
- `core/src/db/write/` owns transactional writes.
- `core/src/db/projection/` owns rebuildable read models.
- `core/src/oplog/` owns metadata synchronization records and replay.
- `core/src/blob_store.rs` owns content-addressed media blobs and thumbnails.

Documentation does not duplicate the full DDL. Read `LIBRARY_DDL` in
`core/src/db/core/schema.rs` when table or index details matter.

## Media Identity

`media_entity` is the logical image or video. It directly references one unique `media_file`, which
stores physical file facts and derivative state. Both logical and physical hashes are unique.

Source-post provenance is stored separately. `subscription_post_member` records the source, post,
item key, page order, URLs, and nullable entity reference. Deleting one entity does not delete its
source-post siblings; provenance remains available to prevent accidental resurrection.

## Lifecycle

`media_entity.status` is the only lifecycle state:

- `0`: Inbox
- `1`: active and visible in `All`
- `2`: Trash

Normal library scopes, folders, smart folders, search, and their counts include active entities
only. Folder membership does not change lifecycle.

## Writes And Projections

User writes run transactionally through the engine and `db/write` modules. A successful write emits
authoritative runtime facts and schedules only the affected projections. Common writes must not scan
or rebuild the whole library.

Roaring bitmap projections accelerate scope and tag intersections. They are caches, never authoring
truth. A projection failure must be repairable from SQLite.

## Schema Policy

Before 1.0 there are no migrations. New libraries are created at `CURRENT_SCHEMA_VERSION`; existing
libraries must match it exactly. Older, newer, malformed, and unknown databases fail clearly and are
not mutated or deleted.

## Library Lifecycle

`core/src/state.rs` owns the open library state. Opening initializes SQLite, blob storage, runtime
services, and background workers. Closing cancels workers, settles their handles, and closes the
database before Electron exits or switches libraries.
