# Duplicates & Merge Behavior

## Overview

Duplicate detection uses perceptual hashing (img_hash DoubleGradient 8x8). Candidate pairs are stored in the `duplicate` table for manual review or automatic resolution.

## Resolution Actions

| Action | Behavior |
|--------|----------|
| `smart_merge` | Picks higher-quality file as winner, merges metadata, deletes loser |
| `keep_left` | Left file wins, merges metadata from right, deletes right |
| `keep_right` | Right file wins, merges metadata from left, deletes left |
| `not_duplicate` | Marks pair as false positive, no files deleted |
| `keep_both` | Dismisses pair, no files deleted |

## What Happens to the Loser

### Free entity (not in a collection)

The loser's `media_entity` is **deleted**. It does not survive the merge. The entity, its file record, and the blob are all removed. Before deletion:

- Tags, source URLs, notes are merged onto the winner
- Rating is consolidated (higher value wins)
- Timestamps are consolidated (earliest `imported_at` and `created_at` are kept)
- Folder memberships and subscription associations transfer to the winner

### Collection member

The loser's `media_entity` **survives** — it stays in its collection. Instead of being deleted, it is **repointed** to the winner's file via `repoint_entity_to_file()`. The flow:

1. Metadata (tags, URLs, notes, rating, timestamps) is merged onto the winner
2. The entity's `entity_file` row is updated to reference the winner's `file_id`
3. The loser's file record and blob are deleted
4. `sync_collection_aggregate_metadata()` re-syncs the collection (cover, tags, counts, rating, status)
5. Folder memberships and subscription associations transfer to the winner

The collection member slot stays intact — it just references the surviving file now.

### Edge cases

- **Loser was the collection cover**: Cover rotates to the next member automatically (`handle_cover_file_deletion`)
- **Collection becomes empty**: Auto-deleted (members are orphaned, collection entity removed)
- **Single-member collection after merge**: Preserved (not auto-collapsed)

## Auto-Merge During Subscription Import

When `auto_merge_enabled` is true, newly imported images are checked against existing files via BK-tree lookup. Auto-merge only fires for **exact perceptual hash matches** (distance = 0) with optional dimension matching. The merge logic is identical to manual resolution — collection membership is handled the same way.

## Key Files

| File | Role |
|------|------|
| `core/src/duplicates/orchestrator.rs` | Merge orchestration, metadata consolidation, collection-aware branching |
| `core/src/sqlite/files.rs` | `delete_file_inner` — deletes file + its media_entity |
| `core/src/folders/collections_db.rs` | `repoint_entity_to_file`, `sync_collection_aggregate_metadata` |
| `core/src/subscriptions/sync_engine/importing.rs` | Auto-merge trigger during subscription import |
