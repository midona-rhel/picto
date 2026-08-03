# PBI-548: Orphaned media entities (ghost items)

## Priority
P1

## Problem
Media entities can end up with no associated file (no `entity_file` row, no `file` row). These ghost items appear in the grid but have no hash, no thumbnail, and can't be deleted, exported, or interacted with. The `delete_entities` command fails silently because it tries to resolve a hash that doesn't exist.

Likely causes:
- Collection creation that partially fails (collection entity created but member assignment errors)
- Split collection leaving the collection entity behind when it has 0 members
- Import pipeline crash between `media_entity` INSERT and `entity_file` INSERT
- `sync_collection_aggregate_metadata` auto-delete check not catching all edge cases

## Implementation

### 1. Startup reconciliation
Add to `reconcile_schema` (runs on every library open):
```sql
DELETE FROM media_entity
WHERE kind = 'single'
  AND entity_id NOT IN (SELECT entity_id FROM entity_file);
```
This removes single entities with no file link. Collections are handled separately (they legitimately have no entity_file row).

### 2. Empty collection cleanup
Also in reconciliation:
```sql
DELETE FROM media_entity
WHERE kind = 'collection'
  AND cached_item_count = 0
  AND entity_id NOT IN (
    SELECT parent_collection_id FROM media_entity
    WHERE parent_collection_id IS NOT NULL
  );
```

### 3. Fix delete_entities
The `delete_entities` command should handle entities that have no file hash — delete the `media_entity` row directly instead of trying to resolve through the hash index.

### 4. Fix bitmap consistency
After cleanup, emit `StatusBatchChanged` so the compiler rebuilds bitmaps without the ghost entities.
