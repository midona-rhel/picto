# PBI-522: Collection entity lifecycle (proper trash/restore/delete)

## Priority
P2

## Problem

Collections are composite entities (a `media_entity` with `kind='collection'` + member items linked via `parent_collection_id`). The entire status lifecycle was designed for single files and has required multiple patches to work for collections. The current state is fragile and needs a proper redesign.

### Current state (quick fixes applied)

**What works (patched):**
- `update_status()` (single file path): checks `find_collection_for_cover_file` and cascades status to collection entity + all member files
- `update_file_status_batch()` (selection path): expands file_ids via `expand_collection_members` to include member files, updates collection entities directly
- `delete_collection()`: clears member `parent_collection_id`/`collection_ordinal` before deleting collection entity (avoids trigger error)
- `remove_collection_members_by_hashes()`: auto-deletes collection when last member is removed
- Bitmap updates cover expanded member file_ids

**What's still fragile:**
1. **Collections don't have their own hash.** The grid uses the cover file's hash. All operations resolve via `file` table which only finds the cover file. The cascade to the collection is implicit and based on `cover_file_id` being set.
2. **`cover_file_id` must be set.** If `sync_collection_aggregate_metadata` hasn't run (or cover changes), the cascade breaks silently.
3. **`sync_collection_aggregate_metadata` re-derives status.** After the cascade sets all members to trash, this sync may re-derive the collection status from member states. Currently works because we cascade first, but the ordering is brittle.
4. **Two different relationship models.** `collection_member` join table vs `parent_collection_id` FK on `media_entity`. These can get out of sync.
5. **Permanent delete from trash.** `delete_files` doesn't know about collections. Permanently deleting a trashed collection's cover file doesn't clean up the collection entity or other members.
6. **`resolve_hash` is file-only.** Collections can never be directly addressed by hash.

### Files with quick-fix patches

| File | Patches |
|------|---------|
| `core/src/sqlite/files.rs:update_status()` | Collection cover check + cascade to entity + members |
| `core/src/sqlite/files.rs:update_file_status_batch()` | `expand_collection_members`, cover collection ID lookup, direct collection entity update |
| `core/src/folders/collections_db.rs:delete_collection()` | Orphan members before delete (trigger fix) |
| `core/src/folders/collections_db.rs:remove_collection_members_by_hashes()` | Auto-delete empty collections |

## Proposed proper solution

### Give collections a real identity
- Option A: Synthetic `file` row per collection (hash = deterministic from entity_id)
- Option B: Extend `resolve_hash` to check `media_entity` as fallback

### Unify relationship model
- Pick ONE of `collection_member` table or `parent_collection_id` FK — not both
- Ensure all queries use the same path

### First-class collection lifecycle
- `trash_collection(id)`: set collection + all members to status=2, update bitmaps
- `restore_collection(id)`: set collection + all members to status=1
- `permanently_delete_collection(id)`: delete collection entity, all member entities, all files, all blobs
- Remove status re-derivation from `sync_collection_aggregate_metadata` when status is explicitly set

### Bitmap tracking
- Collections should have bitmap entries by entity_id
- When status changes, update collection bitmap + all member bitmaps atomically

## Acceptance criteria

- [ ] Trash a collection → collection + all members move to trash (both paths: single + batch)
- [ ] Restore a collection → collection + all members return to active
- [ ] Permanently delete a trashed collection → everything is gone (files, entities, blobs)
- [ ] Inbox reject a collection → collection + all members move to trash
- [ ] Inbox accept a collection → collection + all members move to active
- [ ] Remove all members from collection → collection auto-deletes
- [ ] Trash an image inside a collection → image trashed AND removed from collection
- [ ] Bitmap counts are correct after all operations
- [ ] Sidebar counts are correct after all operations
- [ ] Collections have proper identity for reliable lifecycle operations
- [ ] Single relationship model (not dual collection_member + parent_collection_id)
- [ ] No implicit cascade through cover_file_id — explicit collection-aware operations
