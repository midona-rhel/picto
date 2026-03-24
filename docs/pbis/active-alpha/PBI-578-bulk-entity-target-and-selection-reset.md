# PBI-578: Bulk entity target and selection reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current selection APIs, `SelectionQuerySpec`, explicit-hash vs all-results branching, and controller code paths that still distinguish single vs batch operations too often.

## Lifecycle
- `Implemented` when one canonical bulk-target model exists across backend and frontend for the intended commands.
- `Activatable` when `PBI-567` and `PBI-568` are implemented enough to execute bulk targets correctly, and `PBI-570` is implemented enough to send them from the frontend.
- `Activated` when live selection-driven entity actions use the canonical bulk-target model by default.
- `Legacy removed` when replaced selection-only side paths and giant explicit-hash expansion paths are deleted for that activated slice.

Activation depends on:
- [PBI-567-greenfield-library-database-reset.md](./docs/pbis/active-alpha/PBI-567-greenfield-library-database-reset.md)
- [PBI-568-greenfield-backend-engine-boundary-reset.md](./docs/pbis/active-alpha/PBI-568-greenfield-backend-engine-boundary-reset.md)
- [PBI-570-greenfield-frontend-reset-program-index.md](./docs/pbis/active-alpha/PBI-570-greenfield-frontend-reset-program-index.md)

## Problem
The application still treats “one item,” “many items,” and “select all” as more different than they should be.

Current problems:
- many write surfaces still split single-item and batch behavior unnecessarily
- `SelectionQuerySpec` still acts like a special side path instead of one canonical bulk-target model
- “select all” is special in practice but not modeled cleanly enough as a first-class target
- some controllers still resolve giant selections into explicit hashes too early

## Product model to encode
Bulk targeting should reflect these rules:
- one media entity and many media entities use the same command surfaces
- a single-item action is just a bulk target of size one
- “select all” must be representable without enumerating millions of hashes
- the same target semantics should be reused across status, tags, metadata, folder membership, delete, export, and similar bulk operations

## Locked decisions

### 1. One canonical `EntityTarget`
Most entity write APIs should accept one target model:
- `entity_hashes`
- `query_results`

Do not keep a separate public single-item target shape.

### 2. “Select all” is `query_results`
The backend must be able to act on the full result set of a query without receiving every selected hash.

That means:
- the frontend sends the query definition
- optional excluded hashes are sent when the user deselects a few items from “all results”
- the backend resolves the target lazily and correctly

### 3. Bulk semantics are shared across domains
The same `EntityTarget` model should be reused for:
- metadata patching
- status changes
- tag add/remove
- folder membership add/remove
- delete
- export
- similar future bulk media operations

### 4. Selection is still a UI concept, but not a backend write concept
The UI may keep selection state and selection summary helpers.

But the backend write boundary should consume `EntityTarget`, not a second special-case selection DTO.

## Required target shape

### Public target type
Use a target model such as:

```ts
type EntityTarget =
  | { kind: 'entity_hashes'; entity_hashes: string[] }
  | { kind: 'query_results'; query: EntityViewQuery; excluded_entity_hashes?: string[] | null };
```

### Supporting reads
The frontend may still need:
- selection summary
- bulk preview counts
- “are all current results selected?” helpers

Those are read helpers. They should not force the write surface back into selection-specific DTOs.

## Relationship to other reset PBIs
- PBI-568 defines the engine boundary that should consume `EntityTarget`
- PBI-570 defines the frontend adapter/controller boundary that should construct `EntityTarget`
- PBI-574 export must reuse this target model
- PBI-573 ingest is not a target consumer itself, but downstream actions on imported entities may be

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- one item and many items share the same write surfaces
- “select all” uses `query_results` instead of hash enumeration
- the main bulk entity operations all consume `EntityTarget`
- controllers stop resolving giant selections to explicit hashes unless the operation truly requires it
- selection remains a UI concern, not a second backend write contract

## Tests
Required tests:
- single-entity status change through `entity_hashes`
- multi-entity tag update through `entity_hashes`
- query-results status change without enumerating all hashes
- query-results delete with excluded hashes
- export with query-results target
- controller tests proving large “select all” operations do not require explicit hash enumeration up front
