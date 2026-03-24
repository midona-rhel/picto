# PBI-567: Greenfield library database reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current database backend plus product intent clarified during review. It is intentionally decision-complete, but it is still AI-generated planning. The implementing engineer should improve local naming and factoring where that clearly preserves the same architecture.

## Lifecycle
- `Implemented` when `core/src/db/**` is the real library database boundary with the new schema, write/query/projection split, and migration path.
- `Activatable` when `PBI-568` and the core frontend/backend boundary slice of `PBI-570` are implemented enough to use `LibraryDatabase` in the live core entity flow.
- `Activated` when the core entity path runs through `LibraryDatabase` end to end.
- `Legacy removed` when the replaced old storage path is deleted for that activated slice.

Activation depends on:
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-570-greenfield-frontend-reset-program-index.md](./docs/pbis/active-alpha/PBI-570-greenfield-frontend-reset-program-index.md)

## Problem
The current database backend works, but its structure reflects multiple historical designs at once instead of one clear model.

Current problems:
- authoritative tables, query projections, and derived caches are mixed together in the same modules
- some write paths maintain bitmaps/sidebar rows directly while other paths rely on compiler rebuilds
- `file`, `entity`, `collection`, and `projection` concepts are still blurred in naming and ownership
- old schema compatibility logic still leaks into hot CRUD paths
- comments often explain historical behavior instead of the intended steady-state design
- the rest of the backend still knows too much about storage layout because the database boundary is not strict enough

This PBI is not a cleanup or migration-in-place exercise. It is a greenfield reset of the library database design based on what the application is trying to do now.

The old database is kept only as a migration input. The new runtime code should not preserve legacy schema assumptions, dual-write behavior, or old naming just because it already exists.

This PBI is intentionally paired with the greenfield backend engine boundary reset. PBI-567 defines the canonical storage shape and storage boundaries. The engine PBI defines how the application is allowed to query and change that stored data. They should be implemented as one coherent architecture program, not as unrelated cleanups.

This PBI is specifically about the library database. It does not force every backend subsystem to share that same storage. Separate services may keep their own storage when that better matches their domain, as long as the boundary is explicit.

## Product model to encode
The new database should encode these product rules:
- the library stores media the user can browse, inspect, tag, organize, and export
- the top-level user-visible thing is a `media_entity`
- a `media_entity` is either a `single` or a `collection`
- collections exist to group singles into one visible top-level entity
- collections contain singles only
- tags and folder membership are durable on single entities only
- commands on collections expand to their member singles when the behavior is meant to affect the contained media
- the database is the main source of data for the frontend
- roaring bitmaps are not authoritative data; they are rebuildable derived artifacts used to make reads fast
- the engine should depend on one typed database boundary, not on tables, SQL, or storage details

## Locked decisions
These are not open questions during implementation:

### 1. Main canonical model
Use one main canonical model:
- `media_entity`
- `media_file`
- `single_media_entity`
- `entity_tag`
- `folder`
- `folder_member`
- `smart_folder`
- `subscription`, `subscription_query`, `subscription_entity`
- view/settings tables

Do not reuse the old split of generic `file` helpers doing both write-side and projection work.

### 2. Collections contain singles only
Collections do not contain collections.

That means:
- no nested collection tree
- no hidden recursive expansion rules
- no collection-on-collection membership tables
- a single entity belongs to at most one collection

### 3. Tags are stored on members only
There is only one durable tag storage model.

When a user tags a collection:
- the command expands to member singles
- tag rows are written on those member singles
- the collection entity itself does not store a duplicate tag row for the same semantic tag

Reason:
- one bitmap model
- one smart-folder matching model
- one split/un-collect story
- no duplicated stored data

### 4. Folder membership is stored on members only
When a collection is added to a folder:
- the command expands to member singles
- durable membership is written on member singles
- collection tile visibility is derived from member membership

### 4a. Query-time grouping decides when collections appear in top-level scopes
Folder, tag, and other member-derived scopes must group matching member singles back into their parent collection tile.

Locked rule:
- if one or more members of a collection match a top-level scope, that collection appears once in the grouped result set
- grouped top-level counts count that collection once
- callers do not receive raw member rows unless they explicitly ask for a member-level view later

Reason:
- collections are first-class visible entities
- tag and folder data still belongs on singles
- query callers should not need to understand the storage trick used to keep stored data simple

### 4b. Collection ownership is exclusive
A single `media_entity` may belong to at most one collection.

Locked rule:
- do not duplicate a single entity just to let it appear in another collection
- do not silently move a single entity from one collection to another during unrelated work

Reason:
- entity-owned data like tags, status, rating, notes, and folder membership stays clear
- ownership is easier to reason about
- collections stay simple grouping objects instead of turning into shared-ownership graphs

### 5. User-authored metadata lives on entities
Store user-authored metadata on `media_entity`:
- name
- notes
- rating
- source URLs
- status
- `date_added`, `date_created`, and `date_modified`

### 6. File-intrinsic and analysis data lives on files
Store file facts and analysis data on `media_file`:
- file hash
- `mime_type`
- `size_bytes`
- `pixel_width` / `pixel_height`
- duration/frame/audio facts
- `perceptual_hash`
- dominant color data
- derivative references

### 7. Bitmaps are derived artifacts with a delta log
Roaring bitmaps are not authoritative data.

Chosen model:
- bitmaps are disposable derived artifacts
- bitmap snapshots can be rebuilt from the authoritative tables at any time
- runtime writes append compact bitmap deltas instead of rewriting full snapshot files eagerly
- startup loads the latest snapshot and replays deltas
- if snapshot or delta log is missing or corrupt, full rebuild is valid and supported

This keeps stored data and cache separate while still avoiding expensive full bitmap rewrites on every small change.

### 8. One strict database abstraction layer
Everything above the database must depend on one typed database boundary, not on schema details.

Use one explicit boundary object such as `LibraryDatabase`.

Rules:
- code outside `core/src/db/**` does not issue SQL
- code outside `core/src/db/**` does not know table names
- code outside `core/src/db/**` does not know bitmap storage details
- the engine consumes typed methods and typed result models only

This boundary is required because the storage layer must be replaceable or evolvable later without re-teaching the engine the schema.

### 9. Library storage is not the only allowed backend storage
`LibraryDatabase` owns library data. It is not required to own every backend subsystem's data.

Locked rule:
- media library entities, files, tags, folders, smart folders, and library-facing projections live behind `LibraryDatabase`
- other bounded backend services may keep their own storage when that makes the boundary cleaner
- service-owned storage must stay behind that service's own typed boundary
- the engine may coordinate multiple services, but it must not collapse their internal storage into one blurred module surface

Reason:
- not every subsystem is the library
- clear service ownership is better than one giant mixed database boundary
- future replacement is easier when service boundaries are explicit

## Required database shape

### Authoritative tables
The new authoritative tables should be shaped like this:

#### `media_entity`
- `entity_id`
- `entity_hash`
- `entity_kind` (`single` or `collection`)
- `status`
- `name`
- `notes`
- `rating`
- `source_urls_json`
- `date_added`
- `date_created`
- `date_modified`

For collections, this table also stores materialized aggregate fields:
- `member_count`
- `total_size_bytes`
- `primary_member_entity_id`
- any other clearly product-facing aggregate needed on every read

Those aggregate fields are stored data, not compiler projections. They are maintained transactionally when membership changes.

Rules:
- `primary_member_entity_id` means the current first member by ordinal
- collection reordering updates `primary_member_entity_id` transactionally

For single entities, this table also stores:
- `parent_collection_entity_id` nullable
- `collection_ordinal` nullable

Rules:
- single entities use `parent_collection_entity_id` to point at their owning collection, if any
- collections leave those fields null
- collections do not point directly to files

#### `media_file`
- `file_id`
- `file_hash`
- file/path-independent media facts
- intrinsic media metadata
- derived analysis fields that belong to the physical file

#### `single_media_entity`
- `entity_id`
- `file_id`

This is the only bridge between a single entity and a file.

Locked invariant:
- one physical `media_file` maps to one single `media_entity`
- one single `media_entity` maps to one physical `media_file`
- collections never have a file row through this bridge

Do not keep using a generic bridge that suggests collections and singles attach to files the same way.

#### `entity_tag`
- `entity_id`
- `tag_id`
- `source`
- timestamps if needed

`entity_id` here is always a single entity in the new canonical model.

The tag table model must be explicit:
- `tag`
- `tag_alias`
- `tag_implication`
- `entity_tag`

Do not leave tag structure implicit in implementation code.

#### `folder`
- folder tree and folder-owned settings only

#### `folder_member`
- `folder_id`
- `entity_id`
- `position_rank`

`entity_id` here is always a single entity in the new canonical model.

#### `smart_folder`
- durable definition only
- name
- tree parent/order
- predicate definition
- user-visible settings

Smart-folder membership is not authoritative data.

### Query projections
Read-side query shapes must be explicit projections, not “one DTO that means everything”.

Use:
- `EntityGridItem`
- `EntityDetails`
- `SidebarNode`
- any other explicit query model required by the frontend

Rules:
- grid insertion/update paths consume `EntityGridItem`
- inspector/detail paths consume `EntityDetails`
- a detail read must not be used as a grid payload substitute
- no fake inheritance like “details extends slim”

### Derived artifacts
Derived artifacts live in their own projection layer:
- bitmap snapshots
- bitmap delta log
- sidebar compiled tree/count projection
- smart-folder compiled membership
- any additional rebuildable compiled read structures

Derived artifacts must be deletable without losing authoritative data.

## Module and boundary structure
Replace the current mixed `sqlite/*`, `folders/db.rs`, `smart_folders/db.rs`, `metadata/db.rs` shape with one clear backend boundary.

Use this module structure:
- `core/src/db/core/`
- `core/src/db/types.rs`
- `core/src/db/write/`
- `core/src/db/query/`
- `core/src/db/projection/`
- `core/src/db/migration_legacy/`

### `db/core`
Owns:
- open/init
- reader pool / single writer
- diagnostics
- schema versioning
- migrations
- transaction wrapper
- manifest/publish/version plumbing

### `db/write`
Owns write operations for the authoritative tables only:
- entities
- files
- collections
- tags
- folders
- smart folders
- subscriptions
- settings

Rules:
- may write only the authoritative tables
- may not upsert sidebar projection rows
- may not patch bitmap files directly
- may not contain old-schema compatibility logic

### `db/query`
Owns query/read interfaces only:
- grid reads
- details reads
- sidebar reads
- search reads
- stats reads

Rules:
- read-only
- projection shaping lives here, not in write modules
- no writes

### `db/projection`
Owns derived artifact maintenance:
- bitmap rebuild
- bitmap delta append/replay/compaction
- sidebar projection build
- smart-folder membership compilation

Rules:
- no business writes to the authoritative tables
- receives structured change signals from write operations

### `db/migration_legacy`
Owns the old world only:
- read old schema
- map old stored data into the new schema
- build the new library DB

Rules:
- this is the only place allowed to understand the old schema
- no hot-path runtime code is allowed to depend on legacy tables or legacy naming

## Interface design
Do not introduce a generic repository abstraction.

Use one explicit database boundary object, for example `LibraryDatabase`, with:
- query methods returning projection types
- command methods using one transaction and returning structured change results

Write operations should return structured results like:
- `CollectionMembershipChange`
- `TagWriteChange`
- `FolderMembershipChange`
- `EntityChange`

Those result objects are then consumed by:
- runtime state-change emission
- projection scheduling
- bitmap delta append

That is the required separation:
- transaction changes authoritative data
- write operation returns exact change set
- outer boundary decides how to publish and project it

That abstraction layer is part of the architecture, not an implementation detail. The point is not only cleaner code today. The point is that the engine can later sit on top of a different storage implementation without leaking table structure upward.

## Explicit expansion rules
Expansion must be explicit and centralized.

Use:
- `ExpansionMode::EntityOnly`
- `ExpansionMode::DescendantsOnly`
- `ExpansionMode::EntityAndDescendants`

Defaults for this product model:

### `EntityOnly`
- grid queries
- sidebar counts
- selection identity/cardinality
- reorder
- navigation/history targeting

### `EntityAndDescendants`
- tags add/remove
- status changes
- rating updates
- notes updates
- source URL updates
- folder membership add/remove

### `DescendantsOnly`
- opt-in only
- use only for workflows that intentionally mean “leaf media only”

No command is allowed to expand “because the old helper used to do it”.

## Naming rules
The new backend names must describe what the data is, not how old code happened to reach it.

Use:
- `media_entity`
- `media_file`
- `single_media_entity`
- `entity_tag`
- `folder_member`
- `EntityGridItem`
- `EntityDetails`
- `entity_hash`
- `file_hash`

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

Avoid:
- generic “files” naming in mixed entity code
- “slim” as a concept name
- “metadata projection” for entity-detail payloads if the stored shape is actually file-scoped
- ambiguous “resolve hashes” helpers that hide expansion rules

Comments should explain:
- invariants
- ownership
- why a boundary exists

Comments should not:
- describe stale migration history
- explain obviously named SQL
- defend legacy structure that no longer applies

## Migration and cutover
This is a hard cut.

Implementation steps:
1. Build the new schema and module layout behind a new schema version and new code path.
2. Implement a one-shot importer from old schema to the new authoritative tables.
3. Recompute collection aggregates during import.
4. Do not migrate old projections or old bitmap files.
5. On first open after migration, rebuild projections and bitmap snapshots for the new schema.
6. Remove old runtime code paths after the cutover is stable.

Forbidden:
- no dual-write
- no runtime read branching between old and new schema
- no “temporary” compatibility code in write/query/projection modules

## Acceptance criteria
This PBI is complete only when:
- the new schema encodes the product model above directly
- no hot-path backend code depends on old schema structure
- write operations are separated from query projections
- bitmaps are fully derived and rebuildable from the authoritative tables
- bitmap delta replay works on startup
- grid and detail reads use separate explicit projection types
- collection/tag/folder expansion behavior is explicit and centrally defined
- collection visibility in grouped top-level scopes is explicit and query-owned
- all SQL is contained within `core/src/db/**`
- the rest of the backend talks to storage only through a typed `LibraryDatabase` boundary
- no CRUD path performs schema-introspection compatibility checks
- comments reflect the new architecture instead of documenting legacy baggage

## Tests
Required tests:
- old-schema to new-schema migration test
- single entity insert/read roundtrip
- collection create/add/remove/split roundtrip
- collection aggregate maintenance test
- tag application to single and collection
- folder add/remove for single and collection
- explicit expansion-mode tests
- grid projection vs detail projection contract tests
- bitmap snapshot + delta replay equivalence test
- delete-all-bitmaps and rebuild test
- boundary test preventing direct SQL outside the new DB modules

## Adjacent cleanup expected during implementation
While implementing this PBI, also remove:
- stale comments that explain obsolete structure
- file/entity naming leftovers in the new path
- any remaining direct bitmap/sidebar writes outside `db/projection`
- any remaining read-shape inheritance pretending details and grid items are the same thing
