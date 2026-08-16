# Media Domain

Every imported file becomes one image or video media entity. A source post with several files
creates several independent entities that share source metadata and retain source order.

## One Ingest Path

Manual drag-and-drop, watched folders, subscriptions, and retries normalize into the durable ingest
queue. Queue workers call the same import pipeline; source adapters do not insert library rows.

1. Resolve and validate the source path.
2. Hash the bytes and reuse an existing entity when content already exists.
3. Extract physical metadata and store the content-addressed blob.
4. Insert the direct entity/file relationship and supplied metadata transactionally.
5. Schedule deferred thumbnail, color, perceptual-hash, and AI work.
6. Emit runtime facts for the affected hashes and scopes.

Imports succeed when the media entity is durable. Derivatives are eventually consistent and survive
restart through database-backed work queues.

## Lifecycle

Inbox media is awaiting acceptance. Active media belongs to `All`. Trash media is awaiting deletion
or restoration. Folder membership and source provenance do not alter lifecycle.

Deleting an entity removes its memberships and deletes its physical blob only when no surviving
entity references it. Source-post siblings remain untouched.

## Ownership

- `core/src/ingest_queue.rs`: durable ingest queue and retries
- `core/src/import/pipeline.rs`: canonical import processing
- `core/src/engine/ingest.rs`: ingest behavior orchestration
- `core/src/db/write/entities.rs`: logical entity writes
- `core/src/db/write/files.rs`: physical file writes
- `core/src/blob_store.rs`: content-addressed media and thumbnail storage
- `core/src/background_work.rs`: deferred derivative execution
- `core/src/media_processing/`: metadata and derivative implementations
