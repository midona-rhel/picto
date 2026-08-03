# PBI-568: Greenfield backend engine boundary reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current backend engine surface plus product intent clarified during review. It is intentionally concrete and decision-complete, but it is still AI-generated planning. The implementing engineer should simplify further where that preserves the same API and behavior.

## Lifecycle
- `Implemented` when `core/src/engine/**` exists, canonical engine methods are real, and transport can call them.
- `Activatable` when `PBI-567` is implemented, `PBI-578` bulk-target semantics are implemented for the intended commands, and the core frontend boundary slice of `PBI-570` can call the canonical engine commands.
- `Activated` when the live core entity flow uses canonical engine commands by default.
- `Legacy removed` when old behavior-owning dispatch handlers for that activated slice are deleted.

Activation depends on:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-578-bulk-entity-target-and-selection-reset.md](./docs/pbis/active-alpha/PBI-578-bulk-entity-target-and-selection-reset.md)
- [PBI-570-greenfield-frontend-reset-program-index.md](./docs/pbis/active-alpha/PBI-570-greenfield-frontend-reset-program-index.md)

## Problem
The current backend behavior layer still exposes too much of its storage and transport history instead of one clean application-facing model.

Current problems:
- the public surface is larger than the product model actually needs
- several commands are shaped around files, tables, or legacy transport concerns instead of media-entity intent
- the grid has too many special-case getters instead of one consistent view query
- metadata writes are still spread across multiple behaviors instead of one entity patch model
- asset access is split across thumbnail/original/path-specific endpoints instead of one typed resolver
- naming is not fully aligned with the frontend model
- the backend still makes some callers think in file terms when the product now thinks in entity terms
- the engine boundary is not yet strict enough to cleanly separate app behavior from storage and transport details

This PBI is the engine-side companion to the greenfield database reset. The database PBI defines what canonical data exists. This PBI defines the one application engine that is allowed to talk to that data.

Media delivery is intentionally split out into the greenfield media delivery service PBI. PBI-568 should define backend behavior and public APIs, not absorb asset transport and streaming concerns into the same document.

## Product model to encode
The backend engine should reflect these application rules:
- the frontend mostly works with media entities, not raw files
- the grid is a typed entity view over a scope plus optional filters, sorting, and pagination
- metadata is updated as media-entity metadata, not as separate storage-shaped updates
- assets are resolved by entity and role, not by a mixture of file path helpers
- deferred work is a separate engine concern, not something hidden inside normal entity reads
- naming should match the frontend model and remain stable
- the engine API should survive a future transport change without changing app behavior
- the engine owns behavior, expansion, state-change emission, and projection scheduling
- the engine coordinates bounded services; it does not force every subsystem to be one big database-shaped module
- transport code only deserializes input, calls the engine, and serializes output

### Event aggregation rule
For one completed backend action, the normal path is:
- compute the full committed impact
- merge sub-step impacts into one `ChangeImpact`
- emit one final self-describing `runtime/state_changed`

Do not emit several `runtime/state_changed` events for one logical action just because multiple subsystems were touched.

Additional `runtime/state_changed` events are allowed only when they represent a later distinct async phase, such as:
- compiler/projection follow-up work
- deferred derivative completion
- subscription/import batch completion that commits after the original action

The engine should aggregate first and emit once. Separate later phases should emit their own later events honestly as separate committed work.

## Locked decisions

### 1. One entity-centric engine surface
The engine should expose intent-shaped APIs, not table-shaped APIs.

Locked rule:
- `ApplicationEngine` is the single backend entry point for application behavior above `LibraryDatabase`
- `ApplicationEngine` may depend on multiple typed backend services, not just `LibraryDatabase`

Keep these public categories:
- `query_entity_view(query)`
- `reconcile_entity_view(request)`
- `get_entity_details(entity_hash)`
- `get_entity_grid_items(entity_hashes)`
- `patch_media_entities(target, patch)`
- `set_entity_status(target, status)`
- `apply_entity_tags(target, operation, tags, provenance_mask?)`
- `rename_tag(tag_id, new_name)`
- `merge_tags(from_tag, to_tag)` or equivalent canonical admin route
- `delete_tag(tag_id)`
- `manage_tag_alias(...)`
- `manage_tag_implication(...)`
- `set_tag_site_mask(tag_id, site_mask)`
- `update_folder_membership(target, folder_id, operation)`
- `resolve_entity_asset(entity_hash, role)`
- `get_selection_summary(selection)`
- deferred-work summary/control APIs
- system/settings/library APIs

Subscriptions get their own follow-up PBI. Do not let subscription-specific engine shape bloat this reset.

Concrete engine surface:

```rust
// Entity reads
fn query_entity_view(&self, query: EntityViewQuery) -> Result<EntityViewPage>;
fn reconcile_entity_view(&self, request: EntityViewReconcileRequest) -> Result<EntityViewReconcileResult>;
fn get_entity_details(&self, entity_hash: &str) -> Result<Option<EntityDetails>>;
fn get_entity_grid_items(&self, entity_hashes: &[String]) -> Result<Vec<EntityGridItem>>;

// Entity writes
fn patch_media_entities(&self, target: EntityTarget, patch: MediaEntityPatch) -> Result<EntityChange>;
fn set_entity_status(&self, target: EntityTarget, status: i64) -> Result<StatusChange>;
fn delete_entities(&self, target: EntityTarget) -> Result<EntityChange>;

// Tags / folders
fn apply_entity_tags(&self, target: EntityTarget, operation: TagOperation, tags: &[String], provenance_mask: Option<u64>) -> Result<TagChange>;
fn update_folder_membership(&self, target: EntityTarget, folder_id: i64, operation: MembershipOperation) -> Result<FolderMembershipChange>;

// Assets / selection / deferred work
fn resolve_entity_asset(&self, entity_hash: &str, role: AssetRole) -> Result<EntityAssetResult>;
fn get_selection_summary(&self, target: EntityTarget) -> Result<SelectionSummary>;
fn get_deferred_work_summary(&self) -> Result<DeferredWorkSummary>;
fn retry_deferred_work(&self, entity_hash: &str) -> Result<()>;
```

### 1a. The engine API is independent of transport
The engine surface is an app API, not a desktop IPC API.

Rules:
- the same engine methods and types should be able to sit behind local IPC today or remote transport later
- public types must be serializable and transport-safe
- filesystem paths, SQLite ids, and other storage or host-process details must not leak through the public engine API unless they are true product concepts

The important split is not “frontend process vs backend process”. The important split is “app behavior vs implementation details”.

### 1c. Services are explicit backend boundaries
The reset should use explicit backend services where a subsystem has its own responsibilities or storage.

Examples:
- `LibraryDatabase` for library data
- `MediaDeliveryService` for media delivery and streaming
- `SubscriptionService` for subscription definitions, runs, issues, and recovery behavior

Rules:
- the engine coordinates these services through typed interfaces
- a service may keep its own storage if that makes the subsystem boundary cleaner
- service-owned storage must not leak through the engine API as raw schema details
- do not force unrelated subsystems into `LibraryDatabase` just because they run in the same app

### 1b. `dispatch` is transport, not architecture
`dispatch` is not the main backend behavior layer in this reset.

Locked rule:
- the real behavior layer is `core/src/engine/**`
- `dispatch` may remain as a transport adapter shell
- `dispatch` must stop being the place where application behavior is defined

### 2. One typed grid view query
The primary grid read API is one typed query object:

```ts
type EntityViewQuery = {
  base_scope:
    | { kind: 'system'; key: 'active' | 'inbox' | 'trash' | 'untagged' | 'uncategorized' }
    | { kind: 'folder'; folder_id: number }
    | { kind: 'collection'; collection_id: number }
    | { kind: 'smart_folder'; smart_folder_id: number }
    | { kind: 'similar'; entity_hash: string }
    | { kind: 'search'; text: string };

  filters?: {
    rating?: { min?: number; max?: number };
    colors?: { any_of?: string[] };
    mime_types?: { any_of?: string[] };
    tags?: { all?: string[]; any?: string[]; none?: string[] };
    date_created?: { from?: string; to?: string };
    date_added?: { from?: string; to?: string };
    date_modified?: { from?: string; to?: string };
  };

  sort?: {
    field: 'date_added' | 'date_created' | 'date_modified' | 'rating' | 'name' | 'size_bytes';
    direction: 'asc' | 'desc';
  };

  page: {
    limit: number;
    after?: string;
  };
};
```

Locked decisions:
- views are ad hoc typed query payloads, not persisted backend view records
- the frontend builds these query objects
- pagination is cursor/anchor based, not offset-first
- the cursor is opaque to the frontend

Keep one secondary batch grid-item query:
- `get_entity_grid_items(entity_hashes)`

That secondary query exists only for targeted reconciliation and eager insertion paths. It is not the main way to drive the grid.

### 2a. Query reconciliation is backend-owned truth, not frontend guesswork
The backend should not keep durable per-window frontend session state, but it should own query membership and ordering truth.

Locked rule:
- the frontend owns the current `EntityViewQuery`, current visible window/anchor state, and back/forward state
- the backend owns whether a state change affects that query and how the visible window should settle

Use a typed reconcile API:

```ts
type EntityViewReconcileRequest = {
  query: EntityViewQuery;
  visible_entity_hashes?: string[] | null;
  anchor?: string | null;
  seq?: number | null;
};

type EntityViewReconcileResult =
  | { kind: 'no_change' }
  | { kind: 'patch_rows'; items: EntityGridItem[] }
  | { kind: 'replace_window'; page: EntityViewPage }
  | { kind: 'full_refresh_required' };
```

The exact payload can tighten, but the behavior is locked:
- the frontend must not infer final placement of newly matching entities from events alone
- the backend must answer whether the current visible query/window changed
- deep sorted/paged grids should reconcile through this backend result, not by frontend re-sorting

### 3. One patch model for entity metadata writes
Entity metadata writes become one patch command:

```ts
patch_media_entities(target, patch)
```

Where:

```ts
type EntityTarget =
  | { kind: 'entity_hashes'; entity_hashes: string[] }
  | { kind: 'query_results'; query: EntityViewQuery; excluded_entity_hashes?: string[] | null };

type MediaEntityPatch = {
  name?: string | null;
  notes?: Record<string, string> | null;
  rating?: number | null;
  source_urls?: string[] | null;
};
```

Locked decisions:
- one patch command is the main metadata write model
- metadata is per media entity
- expansion behavior belongs to the backend, not the frontend
- collection-targeted writes use the explicit expansion rules from the database/domain layer
- there is no separate single-item vs multi-item write surface; one item is just `entity_hashes` of length 1
- “select all” is represented as `query_results`, not by enumerating every selected entity hash
- the same `EntityTarget` model should be reused across delete, tags, status, folder membership, export, and similar bulk actions

Likewise:
- tag changes use `apply_entity_tags(target, operation, tags)`
- folder membership uses `update_folder_membership(target, folder_id, operation)`
- status changes use `set_entity_status(target, status)`

### 4. One naming system shared with the frontend
Use these meanings consistently across backend and frontend:
- `date_added`: when the entity entered the library
- `date_created`: original media creation/publication/capture time if known
- `date_modified`: last entity metadata modification time
- `entity_hash`: public stable entity identity
- `file_hash`: physical file content hash
- `media_entity`
- `media_file`
- `EntityGridItem`
- `EntityDetails`

Rules:
- serialized API fields should match the frontend model directly
- do not keep public names like `imported_at` if the meaning is `date_added`
- do not use `slim` as a public engine concept
- do not name public engine methods after storage details

### 5. One typed asset resolver
Replace the public asset/path surface with one entity-centric resolver:

```ts
resolve_entity_asset(entity_hash, role)
```

Use roles:
- `thumbnail`
- `preview_image`
- `original_media`

Locked behavior:
- single image:
  - `thumbnail` = generated thumbnail if present
  - `preview_image` = original image
  - `original_media` = original image file
- single video:
  - `thumbnail` = generated thumbnail
  - `preview_image` = generated thumbnail or best preview frame
  - `original_media` = original video file
- collection:
  - `thumbnail` = primary member thumbnail
  - `preview_image` = primary member best display image
  - `original_media` = primary member original media

The primary member used for all collection asset roles must come from one collection rule. In practice this is the current first member by ordinal, materialized as `primary_member_entity_id`.

Return a typed result:

```ts
type EntityAssetResult = {
  role: 'thumbnail' | 'preview_image' | 'original_media';
  available: boolean;
  url?: string;
  mime_type?: string;
  source_entity_hash?: string;
};
```

Do not assume every entity has every asset role. Absence must be explicit, not a vague path failure.

### 6. Deferred work is a separate engine surface
Deferred work is not part of normal entity reads.

Keep a small dedicated surface:
- `get_deferred_work_summary()`
- `retry_deferred_work(...)`
- optional `cancel_deferred_work(...)`

Rules:
- do not expose queue tables directly
- do not overload entity reads with worker state
- normal entity APIs return current entity state only
- deferred-work APIs return processing state

### 7. Collapse transport-shaped duplication
The new engine API must remove obvious duplicate surfaces.

Examples to remove or fold:
- hash-vs-id public variants of the same entity read
- per-field metadata commands as the main public model
- separate thumbnail/original/path resolvers as the main public model
- special-case grid endpoints for each scope
- duplicated folder membership getters that differ only by how the same entity is addressed

The only acceptable permanent duplication is:
- primary view query vs targeted reconciliation read
- single-entity details vs multi-entity grid items
- task/deferred-work APIs vs normal entity APIs

## Expected backend shape
The engine layer should be organized around application domains, not command-string buckets.

Use this structure:
- `core/src/engine/mod.rs`
- `core/src/engine/reads.rs`
- `core/src/engine/writes.rs`
- `core/src/engine/tags.rs`
- `core/src/engine/folders.rs`
- `core/src/engine/collections.rs`
- `core/src/engine/smart_folders.rs`
- `core/src/engine/assets.rs`
- `core/src/engine/selection.rs`
- `core/src/engine/deferred.rs`
- `core/src/engine/system.rs`
- `core/src/engine/target.rs`

Use these categories:
- entity view/query surface
- entity write surface
- tag/folder/status domain commands
- asset resolution surface
- deferred-work surface
- system/settings/library surface

Use one explicit engine interface above transport, for example `ApplicationEngine`.

Rules:
- transport adapters call the engine interface
- the engine interface calls the database boundary and any other bounded service interfaces
- transport code is not allowed to define app behavior
- the engine is not allowed to reach around service boundaries and access storage details directly
- the engine resolves `EntityTarget`, applies expansion rules, emits state changes, and schedules projection work

### Target resolution belongs in the engine
The engine owns `EntityTarget -> entity_ids` resolution.

Use a dedicated target module for:
- `entity_hashes` -> id lookup
- `query_results` -> execute `EntityViewQuery`, collect entity ids or hashes, apply exclusions
- shared bulk-target behavior reused by status, metadata, tags, folders, delete, export, and similar commands

The legacy dispatcher may still exist as a transport shell, but it should become a thin adapter over the engine API. It must not remain the place where app behavior is defined.

## Implementation phases
### Phase 1: Engine skeleton and target resolution
- create `ApplicationEngine` holding `Arc<LibraryDatabase>`
- implement `target.rs`
- wire `AppState` to hold `Arc<ApplicationEngine>`

### Phase 2: Read surface
- `query_entity_view`
- `get_entity_details`
- `get_entity_grid_items`

### Phase 3: Write surface
- `patch_media_entities`
- `set_entity_status`
- `delete_entities`
- each write resolves target, calls the database boundary, emits state changes, and schedules compiler work

### Phase 4: Domain commands
- `apply_entity_tags`
- `update_folder_membership`
- folder, collection, and smart-folder CRUD through engine-owned domain methods

### Phase 5: Asset resolver
- `resolve_entity_asset`
- collection asset resolution through `primary_member_entity_id`

### Phase 6: Transport adapter rewrite
- rewrite `dispatch/mod.rs` and any remaining transport entry points to call `ApplicationEngine`
- deserialize input
- call engine method
- serialize output
- remove old `dispatch/typed/*` behavior modules

## Relationship to PBI-567
This PBI is intentionally paired with PBI-567.

PBI-567 defines:
- what canonical data exists
- how tables are structured
- where projections and bitmaps live

PBI-568 defines:
- how the application is allowed to query and change that data
- what the backend exposes publicly
- how naming and types line up with the frontend
- where application behavior should live above the database boundary

Do not implement one as if the other did not exist.

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- `core/src/engine/**` exists as the main behavior layer
- the public backend surface is entity-centric rather than file/table-centric
- the main grid read path is one typed `EntityViewQuery`
- metadata writes are patch-based and target-based
- asset access is one typed resolver surface
- deferred work has its own small engine API
- public names match the frontend model
- duplicate transport-shaped APIs are removed or folded into clearer main ones
- the engine exposes one transport-independent app API above the transport layer
- transport adapters are thin shells, not behavior owners
- the dispatch shell, if it still exists, is only an adapter layer and not the main behavior layer
- the engine owns target resolution, expansion, event emission, and projection scheduling

## Tests
Required tests:
- query shape tests for `EntityViewQuery`
- cursor stability tests for entity view pagination
- target resolution tests for `entity_hashes` vs `query_results`
- metadata patch tests for single and collection targets
- tag/folder/status routing tests through target-based engine APIs
- asset resolver tests for single image, single video, and collection cover behavior
- engine-to-database integration tests for state-change emission and projection scheduling
- transport adapter integration tests proving deserialize -> engine -> result serialization flow
- field-naming serialization tests for `date_added`, `date_created`, and `date_modified`
- boundary tests proving file-table operations are not exposed publicly by the engine

## Adjacent cleanup expected during implementation
While implementing this PBI, also remove:
- transport-shaped duplicate commands
- public `slim` naming
- public hash-vs-id duplication where one entity identity form is sufficient
- public asset/path APIs that leak blob/file implementation details
