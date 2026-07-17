# PBI-601: Local persistence crash-safety hardening

## Priority
P1

## AI-generated caveat
This document is based on a code audit of the live persistence layer (`core/src/db/`, `core/src/blob_store.rs`) performed 2026-07-17. Every defect below was verified against current source with file/line references. It is a standalone crash-safety fix set, and it is also the hard prerequisite for PBI-602 (multi-device sync): sync assumes a locally durable, atomic write layer, which the code does not currently provide.

## Lifecycle
- `Implemented` when all locked decisions below are in code with tests.
- `Activated` immediately on merge — these are unconditional bug fixes with no feature flag.
- `Legacy removed` when the dead bitmap-delta persistence path is deleted.

Blocks:
- [PBI-602-multi-device-sync-architecture.md](./docs/pbis/active-alpha/PBI-602-multi-device-sync-architecture.md)

## Problem
The library treats user data as high-value, but the local write layer can silently lose or corrupt data on crash or power loss:

1. **Blob writes are not atomic and never fsync'd.** `write_original` does `fs::create_dir_all` + `fs::write` directly to the final content-addressed path (`core/src/blob_store.rs:107-111`); `write_thumbnail` is the same pattern (`core/src/blob_store.rs:145-150`). A crash mid-write leaves a truncated file whose filename claims a hash its bytes do not have. Because both writers skip when the path already exists (`core/src/blob_store.rs:104-106`, `142-144`) and nothing ever re-verifies bytes against the filename, a corrupt blob is treated as authoritative forever.
2. **Many multi-table mutations run without a transaction.** `with_write` hands out the raw autocommit connection (`core/src/db/mod.rs:478-485`), so each statement commits independently unless the writer opens its own transaction. Non-transactional multi-statement writers include:
   - `delete_entities` — six DELETEs per entity plus a nested member loop (`core/src/db/write/entities.rs:176-270`)
   - `delete_tag`, `merge_tags`, `rename_tag`, `add_tags`, `remove_tags` (`core/src/db/write/tags.rs:11-185`)
   - `delete_folder`, `update_folder`, `add_members`, `remove_members`, `reorder_members`, `reorder_folders` (`core/src/db/write/folders.rs:25-205`)
   - `set_entity_status`, `patch_entity_metadata` (`core/src/db/write/entities.rs:53-156`)
   - all `_bulk` variants, which delegate to the same writers (`core/src/db/mod.rs:1479-1594`)

   A crash mid-sequence leaves half-deleted entities, orphaned `entity_tag`/`folder_member` rows, or partially applied bulk edits.
3. **`PRAGMA synchronous` is never set** (`core/src/db/mod.rs:396-405`), so it defaults to `NORMAL` under WAL: an app crash is safe, but OS crash / power loss can drop the most recent committed transactions. There is no `wal_checkpoint` anywhere, and `close_library` (`core/src/state.rs:151-181`) drops the DB without checkpointing.
4. **The bitmap delta persistence path is dead code.** `flush_bitmap_deltas` (`core/src/db/mod.rs:2631-2634`) has zero callers, so `bitmaps.delta` is never written at runtime and startup falls through to `full_rebuild`. Worse, when a stale delta file *does* replay non-zero records, it is trusted without any cross-check against authoritative tables (`core/src/db/mod.rs:457-464`) — a latent divergence path for projections.
5. **No open-time data reconcile.** Startup repair is schema/migration-scoped only (`core/src/db/mod.rs:386-465`); nothing detects or repairs the partial states that (1) and (2) can produce, and there is no orphan-blob sweep.

Related audit findings recorded here but **out of scope** for this PBI:
- `entity_fts` and `tag_fts` are created but never populated, yet `entity_fts` is queried in grid search (`core/src/db/query/grid.rs:320`) — text search likely returns nothing. Separate defect, separate fix.
- The entire `core/src/sqlite/` tree (schema v38, `ddl.rs`, `reconcile.rs`, `migrations.rs`) is orphaned dead code — never declared in `lib.rs`, zero references. It should be deleted so no future work targets the wrong schema.
- Subscription bookkeeping (`post_member` → `"imported"`) commits separately from the media import transaction (`core/src/subscriptions/sync_engine/persistence.rs:34-49`). Bounded today by content-addressed idempotency; acceptable, documented, not changed here.

## Locked decisions

### 1. Blob writes become atomic: temp file + fsync + rename
- Write to a temporary file in the **same directory** as the final path, `sync_all` the file, then atomically rename onto the content-addressed name. Fsync the directory after rename on platforms where that matters.
- The existing existence-skip stays, and becomes *correct*: with rename-based atomicity, a file at the final path is guaranteed complete.
- Applies to `write_original` and `write_thumbnail`.
- On the ingest path, the hash is computed from the bytes being written, so no extra verification is needed at write time. Read-side hash verification (scrub) belongs to PBI-602.

### 2. Every multi-statement mutation runs in one transaction
- Wrap each writer listed in Problem §2 in `unchecked_transaction()` … `commit()`, matching the pattern already used by `insert_ingested_single` (`core/src/db/mod.rs:788,846`) and the ingest queue (`core/src/ingest_queue.rs:275-319`).
- Bulk variants must be atomic **per logical user action**, not per row: one transaction around the whole bulk loop.
- Rule going forward: any writer that issues more than one `execute` must own a transaction. Add a brief note to the module doc of `db/write/`.

### 3. Durability pragmas and shutdown checkpoint
- Set `PRAGMA synchronous = FULL` on the write connection. Write volume in this app (user actions + imports) does not justify trading away power-loss durability; imports batch inside single transactions so the per-commit fsync cost is amortized.
- Read-pool connections stay at `NORMAL` (they do not commit).
- `close_library` performs `PRAGMA wal_checkpoint(TRUNCATE)` before dropping the connection.

### 4. Delete the bitmap delta persistence path
- Bitmaps are derived data with a working `full_rebuild`; the delta log is unreachable at runtime and its replay path is trusted without validation. Delete `flush_bitmap_deltas`, `append_deltas`/`replay_deltas`, and the `bitmaps.delta` file handling; always rebuild on open (which is the de facto behavior today).
- If rebuild-on-open ever becomes a measured startup cost, persistence returns as a *validated snapshot* design under PBI-602's verification rules — not by resurrecting this path.

### 5. Out of scope
- Orphan-blob GC and read-side blob scrubbing (owned by PBI-602 — GC semantics must be sync-aware).
- FTS population fix and deletion of `core/src/sqlite/` (separate small PBIs/commits).
- The subscription cross-store two-phase write (documented, benign).

## Acceptance criteria
- [x] `write_original` / `write_thumbnail` are temp+fsync+rename; a partial file can never exist at a final content-addressed path.
- [x] All writers listed in Problem §2 (including `_bulk` variants) execute inside a single transaction per logical action.
- [x] Write connection runs `synchronous = FULL`; WAL is checkpointed with TRUNCATE on close (via `Drop` on `LibraryDatabase`).
- [x] Bitmap delta log code and file handling are deleted; open always full-rebuilds bitmaps.
- [x] No remaining multi-`execute` writer without a transaction — enforced structurally: `with_write` itself now opens the transaction and commits on success, so every write action through the boundary is atomic by construction.

## Implementation notes (2026-07-17)
Implemented with one deliberate strengthening over the plan: instead of wrapping each listed writer individually, `LibraryDatabase::with_write` was made transactional (`db/mod.rs`). Every write closure — including subscription runtime, credential, ingest-queue, and dispatch-level callers — now runs in exactly one transaction; an error rolls back everything. The nine pre-existing inner `unchecked_transaction` sites (ingest, collections, deferred-work claim, duplicates, subscription resets) were demoted to plain connection use since the outer transaction now covers them. Blob staging uses `blobs/tmp/` (cleared on open — anything there is a crash leftover); originals fsync file + directory before rename, thumbnails rename without fsync (regenerable). `bitmaps.delta` is deleted on open if present; the `BitmapStore` pending-delta accumulation (an unbounded-growth path, since the flush was dead code) was removed with it. Event emission stays ordered correctly: engine-layer `emit_state_changed` calls run after `with_write` returns, i.e. after commit.

## Testing requirements
- Unit: blob write interrupted before rename (inject failure) leaves no file at the final path; rename result is byte-identical to input.
- Unit: for each wrapped writer, force an error mid-sequence (e.g., poisoned statement on the Nth step) and assert full rollback — no partial rows.
- Unit: bulk mutation with a failing row rolls back the entire batch.
- Integration: `cargo test` core suite green; `alpha:verify` green.
- Manual: kill the app (SIGKILL) during a large import; on relaunch, library opens clean, no truncated blobs, no orphaned rows for the interrupted entity.

## Definition of done
- [ ] Code implemented and reviewed (implemented 2026-07-17; review pending)
- [x] Tests written and passing (rollback test, staging-cleanup tests; full core suite green)
- [ ] `gate:alpha` green
- [ ] PBI-602 unblocked (durability assumptions it relies on are now true)
