# PBI-539: Persistent download queue with partial collection recovery

## Priority
P2

## Problem
Subscription downloads are entirely in-memory. If the app crashes, gallery-dl is killed, or the user quits mid-download, all pending collection members are lost — files already downloaded to the temp directory are never imported, and partially-assembled collections vanish. The user has no visibility into this data loss and no way to recover.

Specific failure modes:
1. **App crash / force quit**: In-memory `PendingCollection` structs are lost. Downloaded files in the temp directory are orphaned and eventually cleaned up by the OS.
2. **gallery-dl crash**: The finalization loop imports whatever was received, but if the process died mid-post, some pages are missing with no way to retry just the missing pages.
3. **Network interruption**: Same as gallery-dl crash — partial data, no resume.
4. **User clicks Stop**: Currently drops all stashed files. The user may have intended to pause, not discard.

## Scope
- `core/src/subscriptions/sync_engine/mod.rs` — download orchestration
- `core/src/subscriptions/sync_engine/importing.rs` — `materialize_collection`
- `core/src/sqlite/schema/ddl.rs` — new table(s)
- Settings UI — storage visibility panel

## Implementation

### 1. Persistent download queue table
```sql
CREATE TABLE IF NOT EXISTS download_queue (
    queue_id        INTEGER PRIMARY KEY,
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id),
    post_id         TEXT NOT NULL,
    category        TEXT NOT NULL,
    preferred_name  TEXT,
    expected_count  INTEGER,
    status          TEXT NOT NULL DEFAULT 'pending',  -- pending, importing, complete, stale
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS download_queue_item (
    item_id     INTEGER PRIMARY KEY,
    queue_id    INTEGER NOT NULL REFERENCES download_queue(queue_id) ON DELETE CASCADE,
    blob_hash   TEXT,           -- NULL until file is hashed and stored
    file_path   TEXT NOT NULL,  -- temp path (or blob path once stored)
    page_num    INTEGER,
    metadata    TEXT,           -- JSON-serialized ParsedMetadata
    status      TEXT NOT NULL DEFAULT 'downloaded',  -- downloaded, prepared, committed
    created_at  TEXT NOT NULL
);
```

### 2. Download flow changes
- When gallery-dl delivers a file, write a `download_queue_item` row instead of (or in addition to) the in-memory stash.
- `prepare_file` updates the row with `blob_hash` and sets status to `prepared`.
- `import_collection_batch` sets all items to `committed` and the queue entry to `complete`.
- On app restart, scan for `pending` / `prepared` queue entries and offer to resume or discard.

### 3. Cleanup policy
- Queue entries with status `complete` are deleted after successful import.
- Queue entries older than N days (configurable, default 7) with status `pending` or `stale` are flagged for cleanup.
- A background worker periodically checks for stale entries and cleans up associated temp files / orphaned blobs.

### 4. Storage visibility
- Settings panel shows: number of pending downloads, total size of temp/orphaned data.
- Action buttons: "Resume pending downloads", "Clean up stale data".
- Surface partial collections in the sidebar or a dedicated "Downloads" view.

## Acceptance Criteria
1. Interrupted downloads survive app restart — queue entries persist in SQLite.
2. Resuming a subscription re-checks the queue and skips already-prepared files.
3. Stale queue entries are cleaned up after the configured retention period.
4. User can see pending/stale download data in settings and manually trigger cleanup.
5. Stop button marks queue entries as `stale` rather than deleting them.

## Test Cases
1. Start a subscription → kill the app mid-download → relaunch → verify queue entries exist and can be resumed.
2. Start a subscription → press Stop → verify queue entries are marked stale, not deleted.
3. Wait past retention period → verify background cleanup removes stale entries and orphaned blobs.
4. Import a 27-image collection → verify all queue items transition through downloaded → prepared → committed.
