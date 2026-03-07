# PTR Domain

## Purpose

The PTR (Public Tag Repository) provides community-sourced tag data for files. Picto syncs with a Hydrus-compatible PTR server to overlay external tags onto locally stored files.

## Architecture

The PTR uses a separate SQLite database (`sqlite_ptr/`) from the main library database. This separation allows the PTR database to be rebuilt or replaced without affecting user data.

## Sync Modes

### Bootstrap

Initial population from a PTR snapshot dump. Imports millions of tag mappings from a pre-built snapshot directory. Used for first-time setup or full rebuild.

### Delta Sync

Incremental updates from the PTR server. Fetches update files since the last sync cursor, processes content updates (tag additions/removals), and advances the cursor.

## Data Flow

1. **PTR server** → raw update content types (tag definitions, mappings, parents, siblings)
2. **Delta processing** → updates stored in PTR database tables
3. **Overlay compilation** — compiler resolves PTR tags for files in the local library:
   - Looks up file hashes in PTR database
   - Resolves tag parents and siblings for display
   - Caches results in PTR overlay for fast lookup
4. **Negative cache** — hashes confirmed to have no PTR tags are cached to skip future lookups

## Compiler Integration

- `PtrSyncComplete { changed_hashes }` — incremental overlay rebuild for changed files only
- `PtrFullRebuild` — full overlay recomputation (manual maintenance)

## Key Files

- `core/src/ptr_controller.rs` — sync orchestration, scheduling, cancellation
- `core/src/ptr_sync.rs` — delta sync engine
- `core/src/ptr_client.rs` — HTTP client for PTR server
- `core/src/ptr_types.rs` — protocol data types
- `core/src/sqlite_ptr/mod.rs` — PTR database handle
- `core/src/sqlite_ptr/bootstrap.rs` — snapshot importer
- `core/src/sqlite_ptr/sync.rs` — sync cursor management
- `core/src/sqlite_ptr/overlay.rs` — compiled overlay (hash → resolved tags)
- `core/src/sqlite_ptr/cache.rs` — negative miss cache
- `core/src/sqlite_ptr/tags.rs` — PTR tag lookup and resolution
