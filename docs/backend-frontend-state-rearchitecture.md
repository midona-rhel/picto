# Backend/Frontend State Re-Architecture

## Goal

Replace the current ad hoc mutation + invalidation model with a single typed
runtime contract where:

1. the backend is the source of truth for what changed,
2. the frontend has one place that interprets that change,
3. components do not manually coordinate cache busting, sidebar refreshes,
   selection invalidation, or inspector refreshes,
4. command and event payloads are typed end to end.

This document is based on the current code in:

- `core/src/events.rs`
- `core/src/dispatch/`
- `core/src/state.rs`
- `src/stores/eventBridge.ts`
- `src/domain/actions/mutationEffects.ts`
- `src/stores/domainStore.ts`
- `src/stores/cacheStore.ts`
- `src/hooks/useInspectorData.ts`
- `src/controllers/folderController.ts`
- `src/controllers/smartFolderController.ts`
- `src/desktop/api.ts`

## Diagnosis

### 1. Mutation meaning is split across backend and frontend

The backend emits `state-changed` plus a few secondary events from
`core/src/events.rs`. That payload contains broad invalidation hints:

- `invalidate.sidebar_tree`
- `invalidate.grid_scopes`
- `invalidate.selection_summary`
- `invalidate.metadata_hashes`

The frontend then reinterprets those hints again in
`src/stores/eventBridge.ts`.

On top of that, controllers also do direct refresh work:

- `src/controllers/folderController.ts`
- `src/controllers/smartFolderController.ts`
- `src/domain/actions/mutationEffects.ts`

That means one mutation can fan out through three different mechanisms:

1. command return path,
2. emitted backend events,
3. direct frontend refresh calls.

That is why the system feels like a sinking ship. The same mutation semantics
are encoded multiple times.

### 2. The frontend owns too much invalidation knowledge

Examples:

- `eventBridge.ts` decides when a grid refresh should happen for active scope
- `mutationEffects.ts` directly invalidates sidebar, selection, and cache store
- `useInspectorData.ts` has custom refresh behavior separate from the main event
  path
- some mutations still rely on controller-local refresh helpers rather than the
  event bridge

This is the exact failure mode behind cases like:

- file moved into auto-tagged folder, but inspector tags do not refresh
- folder/sidebar/grid counts desynchronize
- one path updates metadata cache, another only bumps grid refresh
- background operations and foreground operations do not share one contract

### 3. State is stored by view concern, not by domain entity

Current stores are organized as separate concerns:

- `domainStore`: sidebar tree + counts
- `cacheStore`: metadata cache + grid refresh sequence + active scope
- `selectionController`: summary cache invalidation
- local hooks/components: inspector state, grid reload, flow refresh tokens

This makes view behavior fast to patch but hard to reason about. The app has no
single runtime model of:

- which entities changed,
- which scopes include those entities,
- which read models depend on those changes,
- which UI surfaces must refresh.

### 4. The command surface is still stringly typed

`src/desktop/api.ts` and `core/src/dispatch/` still agree by convention.

Problems:

- command names are strings
- args are destructured from `serde_json::Value`
- return contracts are only partially centralized in TypeScript
- event payloads and command payloads are not generated from one source

This makes drift inevitable.

### 5. The system is mostly invalidate-and-refetch

The current model is closer to:

- "something changed, refresh a few stores"

than:

- "this entity changed, and these read models depend on that entity"

That leads to both under-invalidation and over-invalidation.

## Design Principles

1. One mutation contract.
2. One event ingestion path.
3. One typed command/query/event schema.
4. Backend computes domain facts.
5. Frontend computes view refreshes from one central dependency graph.
6. Components never manually invalidate global state.
7. Background jobs and direct user actions use the exact same delivery model.

## Target Model

Use three layers:

1. Typed Command/Query Contract
2. Mutation Receipt
3. Resource Graph

### 1. Typed Command/Query Contract

Every command and query must be defined once in Rust and surfaced to
TypeScript from that schema.

Required direction:

- Rust enums/structs for commands, queries, and events
- generated TypeScript request/response/event types
- one invocation wrapper that only accepts declared commands

This is the work tracked by `PBI-234`, but it should be expanded from "typed
dispatch" into the foundation for runtime state synchronization too.

### 2. Mutation Receipt

Every successful mutation should produce a typed receipt.

Suggested shape:

```ts
type MutationReceipt = {
  mutation_id: string;
  seq: number;
  committed_at: string;
  origin: MutationOrigin;
  facts: MutationFacts;
  derived: DerivedInvalidation;
};
```

`facts` are the important part. These should be domain facts, not UI hints.

Suggested `MutationFacts`:

```ts
type MutationFacts = {
  entity_ids_changed?: number[];
  entity_hashes_changed?: string[];
  entity_membership_changed?: number[];
  folder_ids_changed?: number[];
  smart_folder_ids_changed?: string[];
  collection_ids_changed?: number[];
  tag_ids_changed?: number[];
  statuses_changed?: Array<{
    entity_id: number;
    from: 'tentative' | 'active' | 'trashed';
    to: 'tentative' | 'active' | 'trashed';
  }>;
  folder_membership_changes?: Array<{
    entity_id: number;
    folder_id: number;
    added: boolean;
  }>;
  selection_affecting_change?: boolean;
  sidebar_structure_changed?: boolean;
  settings_changed?: string[];
};
```

`derived` may still exist, but only as an optimization, not as the primary
truth.

Suggested `DerivedInvalidation`:

```ts
type DerivedInvalidation = {
  sidebar_tree?: boolean;
  sidebar_counts?: boolean;
  scope_keys?: string[];
  metadata_entities?: number[];
  selection_summary?: boolean;
};
```

The backend emits one event:

- `mutation-committed`

The invoke response for a mutation should optionally include the same receipt.
That allows the frontend to process foreground and background changes through the
same reducer.

### 3. Resource Graph

The frontend should not directly manage "refresh sidebar + invalidate cache +
bump grid + maybe selection".

Instead, keep a central runtime dependency graph over read resources:

- `sidebar_tree`
- `sidebar_counts`
- `grid:<scope_key>`
- `metadata:<entity_id>`
- `selection:<selection_key>`
- `collection:<entity_id>`
- `tasks`

Each resource has:

- key
- epoch/version
- dependencies
- stale/exact state
- fetch function

Then the event bridge becomes a reducer:

```ts
onMutationCommitted(receipt) -> mark resources stale -> schedule fetches
```

Not:

```ts
if sidebar then refresh sidebar
if grid then clear cache
if metadata then maybe patch inspector
```

## Concrete Communication Design

## A. Commands

Commands mutate domain truth and return:

```ts
type CommandResult<T> = {
  data: T;
  receipt?: MutationReceipt;
};
```

Foreground path:

1. user action dispatches typed command
2. backend commits mutation
3. backend returns typed result + receipt
4. backend also broadcasts `mutation-committed`
5. frontend dedupes by `mutation_id`

Background path:

1. worker commits mutation
2. backend emits `mutation-committed`
3. same reducer handles it

This removes the distinction between:

- "mutation from button click"
- "mutation from subscription"
- "mutation from compiler batch"

## B. Queries

Queries do not mutate. They return typed read models only.

Do not emit `state-changed` from queries.

Queries should be normalized around read resources:

- `get_sidebar_tree()`
- `get_sidebar_counts()`
- `get_grid_snapshot(query_key, cursor)`
- `get_entity_metadata_batch(entity_ids | hashes)`
- `get_selection_summary(selection_key)`
- `get_collection_summary(collection_id)`

The frontend should fetch these via one resource store, not directly from each
component/hook.

## C. Events

Keep only these app-wide runtime events:

1. `mutation-committed`
2. `task-progress`
3. `task-finished`
4. `library-switching`
5. `library-switched`
6. `ptr-sync-*` and other long-job status events

Delete or phase out:

- `state-changed`
- `sidebar-invalidated`
- `grid-snapshot-invalidated`

Those are intermediate invalidation transports, not real domain events.

## D. Backend Responsibility

The backend must compute mutation facts centrally, not each dispatch module by
hand.

Today, each dispatch module hand-builds `MutationImpact`. That is brittle and
already duplicated across:

- `core/src/dispatch/files_lifecycle.rs`
- `core/src/dispatch/files_metadata.rs`
- `core/src/dispatch/tags.rs`
- `core/src/dispatch/folders.rs`
- `core/src/dispatch/subscriptions.rs`

Instead, mutations should use one helper layer:

```rust
let facts = MutationFactsBuilder::new()
    .entity_changed(entity_id)
    .folder_membership_changed(entity_id, folder_id, true)
    .status_changed(entity_id, from, to)
    .build();

emit_mutation_committed(origin, facts);
```

`DerivedInvalidation` can then be computed in one place from those facts.

That should replace most ad hoc `MutationImpact::file_lifecycle`,
`MutationImpact::file_metadata`, `.grid_all()`, `.selection_summary()`, and the
rest of the builder chain.

## E. Frontend Responsibility

The frontend must stop encoding mutation fanout in controllers.

Delete or phase out:

- `src/domain/actions/mutationEffects.ts`
- controller-level refresh helpers in folder/smart folder/etc.
- direct `invalidateAll() + bumpGridRefresh()` from arbitrary features

Replace with one `runtimeSyncStore`:

```ts
type RuntimeSyncStore = {
  receiptsSeen: Set<string>;
  resourceEpochs: Map<ResourceKey, number>;
  staleResources: Set<ResourceKey>;
  applyReceipt(receipt: MutationReceipt): void;
  invalidateResource(key: ResourceKey): void;
  scheduleResourceFetch(key: ResourceKey): void;
};
```

Components subscribe to resources, not to mutations.

Examples:

- sidebar subscribes to `sidebar_tree`
- grid subscribes to `grid:<active_scope>`
- inspector subscribes to `metadata:<selected_entity>`
- selection summary subscribes to `selection:<selection_key>`

## How The Folder Auto-Tag Example Should Work

Current bad model:

1. add to folder
2. backend adds folder membership
3. backend separately applies auto-tags
4. frontend may refresh grid but inspector metadata stays stale unless another
   invalidation path is added

Target model:

1. `AddEntityToFolder` command commits
2. backend applies folder membership + auto-tags in one transaction
3. backend emits one receipt:

```ts
{
  facts: {
    entity_ids_changed: [123],
    folder_ids_changed: [9],
    folder_membership_changes: [{ entity_id: 123, folder_id: 9, added: true }],
    selection_affecting_change: true
  }
}
```

4. runtime sync reducer sees:
   - selected entity 123 metadata resource is stale
   - active grid scope may be affected
   - sidebar tree/counts may be affected
5. reducer marks those resources stale
6. subscribed views refetch automatically

No controller-local special case. No extra inspector patch path.

## How Delete/Restore Should Work

Backend receipt contains:

```ts
{
  facts: {
    entity_ids_changed: [123],
    statuses_changed: [{ entity_id: 123, from: 'active', to: 'trashed' }],
    folder_ids_changed: [4, 7],
    selection_affecting_change: true
  }
}
```

Frontend dependency rules know:

- `system:all`, `system:trash`, `system:inbox`, `system:uncategorized`,
  `system:untagged`, affected folder scopes, and smart folders may all be stale
- selected entity metadata is stale
- sidebar tree/counts are stale

Again: one reducer, not manual scattered invalidation calls.

## Migration Plan

## Phase 0: Freeze new patchwork

1. Stop adding new controller-local invalidation helpers.
2. Keep `eventBridge.ts` as the only event consumer during migration.
3. Do not add new ad hoc events unless they are long-running task progress.

## Phase 1: Typed Contract

Implement `PBI-234` first.

1. Define typed command/query/event structs in Rust.
2. Generate TypeScript types.
3. Keep legacy string dispatch as compatibility wrapper during migration.
4. Migrate one vertical slice first:
   - folders
   - file lifecycle

## Phase 2: Mutation Receipt

1. Add `mutation-committed`.
2. Implement central `MutationFactsBuilder`.
3. Emit receipt from a small set of domains first:
   - folders
   - file lifecycle
   - tags

Keep legacy `state-changed` during the migration.

## Phase 3: Runtime Sync Store

1. Build `runtimeSyncStore`.
2. Make `eventBridge.ts` feed only that store.
3. Replace:
   - `mutationEffects.ts`
   - direct sidebar refresh calls
   - direct cache invalidation from controllers

## Phase 4: Resource Stores

Move major UI surfaces to resource subscriptions:

1. sidebar
2. active grid
3. inspector metadata
4. selection summary

At this point, controllers should stop doing post-command refresh work.

## Phase 5: Remove Legacy Invalidation Events

Delete:

- `state-changed`
- `sidebar-invalidated`
- `grid-snapshot-invalidated`

after all active surfaces use `mutation-committed`.

## Recommended First Slice

Start with folders + file lifecycle.

Why:

1. this is where the current invalidation pain is most visible
2. it exercises:
   - grid membership changes
   - metadata changes
   - sidebar count/tree changes
   - selection effects
3. it will force the architecture to handle the hard cases early

Concrete first commands:

- `add_file_to_folder`
- `add_files_to_folder_batch`
- `remove_file_from_folder`
- `update_file_status`
- `delete_file`

## Immediate Cleanup Actions

Before full migration:

1. Remove `applyMutationEffects()` usage from controllers once equivalent
   receipt-driven handling exists.
2. Make `eventBridge.ts` the only runtime invalidation ingress.
3. Stop directly calling `fetchSidebarTree()` from mutation facades.
4. Centralize active-scope dependency logic in one module instead of leaving it
   in `eventBridge.ts`.

## Recommended PBIs

The current backlog already contains the right backbone:

1. `PBI-234` — typed dispatch contract
2. `PBI-235` — deduplicate mutation impact construction

I recommend adding a new architecture PBI after those:

- `Runtime sync store and mutation receipt migration`

That work is larger than a comment on `PBI-234` and should be tracked
explicitly.

## Bottom Line

The application does not primarily have a "missing invalidation" problem.

It has a **missing mutation model** problem.

Right now:

- backend emits partial hints,
- frontend reinterprets them,
- controllers patch over the gaps,
- components still own refresh logic.

The fix is:

1. typed commands,
2. one committed mutation receipt,
3. one frontend reducer for receipts,
4. resource-based subscriptions for UI surfaces.

That is the path that stops this from being a permanent patch loop.
