# Shared Folders and Shared Bulk Summary Semantics

## Problem

The rebuilt app already uses canonical `EntityTarget` for bulk edits, but the current `SelectionSummary` thinking is still drifting toward the old selection/bitmap path.

That is the wrong direction.

The live rebuilt app needs one summary model that matches the same target model used by bulk writes:
- explicit multi-select
- query-results select-all
- optional exclusions

The summary surface should support:
- shared tags
- shared folders
- shared rating
- total size and other aggregate stats

without expanding giant query targets into frontend hash arrays.

## Correct model

`SelectionSummary` is a read helper over the same canonical bulk target that mutations use.

It should be built from:
- `EntityTarget { kind: 'entity_hashes' }`
- `EntityTarget { kind: 'query_results', query, excluded_entity_hashes }`

It should not reintroduce:
- `SelectionQuerySpec`
- old bitmap-only summary ownership
- frontend-side giant hash expansion

## Desired behavior

### Shared folders
`SelectionSummary.shared_folders` should list only the folders that every selected entity belongs to.

These are the safe removable folder chips in the multi-select inspector.

### Shared tags
`SelectionSummary.shared_tags` should continue to mean tags present on every selected entity.

These are the safe removable tag chips in the multi-select inspector.

### Rating
The summary should expose enough aggregate rating information to support honest bulk editing:
- if all selected entities share one rating, show it as shared
- otherwise show mixed state

The current `rating_stats.shared` shape is acceptable for that.

### Notes
Do not try to compute “shared notes” for bulk summary.

Bulk notes should remain a write surface:
- user enters notes in bulk mode
- the value is applied to the target

No shared-notes intersection is required.

## Backend design

### 1. Keep `SelectionSummary` on the canonical `EntityTarget` path
The live summary path should stay behind:
- [core/src/engine/selection.rs](./core/src/engine/selection.rs)
- [core/src/db/mod.rs](./core/src/db/mod.rs)

Do not add new logic to:
- [core/src/selection/summary.rs](./core/src/selection/summary.rs)

That file is legacy-shaped and should not become the rebuilt app authority.

### 2. Use the same bulk target semantics as writes
For `query_results` targets:
- use the same canonical query definition
- respect `excluded_entity_hashes`
- do not enumerate all hashes in frontend memory

The summary query should operate over the same DB-side bulk target / target-resolution model used by:
- `patch_media_entities`
- `apply_entity_tags`
- `set_entity_status`
- `update_folder_membership`

### 3. Compute intersections in SQL
For large selections and query targets, compute summary state in SQL.

Canonical examples:
- shared tags:
  - `GROUP BY tag_id`
  - keep tags where membership count equals selected count
- shared folders:
  - `GROUP BY folder_id`
  - keep folders where membership count equals selected count
- rating:
  - `MIN(rating)`, `MAX(rating)`, optional count stats
- total size:
  - `SUM(size_bytes)`

The summary should be DB-owned truth, not a frontend reduction over loaded items.

### 4. Shared state is for safe removal and truthful display
Intersections are needed for:
- removable shared tag chips
- removable shared folder chips
- truthful “shared vs mixed” display

Intersections are not required for additive actions.

Bulk add/remove semantics should be:
- add tag: always allowed
- remove tag: from shared tags
- add folder: always allowed
- remove folder: from shared folders
- set rating: always allowed

## API shape

### Add shared folders to `SelectionSummary`
The canonical summary DTO should include:

```rust
pub struct SelectionFolderInfo {
    pub folder_id: i64,
    pub name: String,
}

pub struct SelectionSummary {
    // existing fields
    pub shared_folders: Vec<SelectionFolderInfo>,
}
```

The TS contract should mirror that.

### Keep `SelectionSummary` a read helper only
It is a read-side convenience for bulk inspector surfaces.

It must not become a second write contract.

The write contract remains canonical `EntityTarget`.

## Frontend behavior

The multi-select inspector should:
- show shared tags as removable chips
- show shared folders as removable chips
- show mixed/shared rating honestly
- show select-all/query-results state explicitly

It should not:
- fall back to scope mode
- pretend loaded-item state is the full query state

## Tests

Required tests:
- explicit multi-select with one shared folder and one non-shared folder
- query-results select-all with exclusions still computes correct shared folders
- shared tag computation stays correct for explicit and query-results targets
- rating summary stays honest for mixed vs shared values
- bulk remove folder uses canonical `update_folder_membership`
- bulk remove tag uses canonical `apply_entity_tags`
- giant query summary does not require frontend hash expansion
