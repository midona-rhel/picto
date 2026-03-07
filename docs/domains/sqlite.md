# SQLite Domain

## Purpose

The SQLite layer provides all persistent storage for the library database. It wraps rusqlite (synchronous) in async-friendly methods via `spawn_blocking`, manages a connection pool for concurrent reads, and coordinates derived artifact compilation.

## Connection Model

- **Write connection** (`with_conn`, `with_conn_mut`): Single `Arc<Mutex<Connection>>`. All writes are serialized through this mutex. `with_conn_mut` provides `&mut Connection` for transactions.
- **Read pool** (`with_read_conn`): `num_cpus` (capped 2–8) read-only connections opened with `SQLITE_OPEN_READ_ONLY`. Round-robin distribution. Writes on these will fail at the SQLite level.
- **WAL mode**: Applied via `apply_pragmas` — enables concurrent reads during writes.

## Schema & Migrations

- `CURRENT_VERSION` in `schema.rs` tracks the schema version.
- New databases: `init_schema()` runs `LIBRARY_DDL` (full DDL).
- Existing databases: `run_migrations(conn, from_version)` applies incremental `if from_version < N` blocks.
- `reconcile_schema()` heals known drift cases even when the version number is current.
- Always add both DDL changes AND a migration block when modifying the schema.

## Bitmap Store

Roaring bitmaps accelerate set membership queries (status checks, tag membership, folder membership, smart folder results).

### BitmapKey Types

| Key | Contains | Purpose |
|-----|----------|---------|
| `Status(n)` | file_ids with status n | 0=inbox, 1=active, 2=trash |
| `AllActive` | Status(0) \| Status(1) | All non-trash files. Used as the universe for search/filter scoping. |
| `Tag(id)` | file_ids directly tagged with tag_id | Direct tag membership |
| `ImpliedTag(id)` | file_ids inheriting tag_id via parents | Parent-chain inheritance |
| `EffectiveTag(id)` | Tag(id) \| ImpliedTag(id) | What the user sees — direct + inherited |
| `Folder(id)` | file_ids in folder | Folder membership |
| `SmartFolder(id)` | Compiled predicate result | Cached smart folder membership |
| `Tagged` | Union of all tagged file_ids | Used to compute "untagged" = AllActive - Tagged |

### Persistence

Bitmaps are persisted to a versioned sidecar file (`bitmaps.vN.bin`). The `Manifest` tracks which version is current. On startup, stale artifact files are pruned. Bitmaps are fully rebuildable from SQL via the compiler.

## Compiler System

The compiler is a background task that reacts to data mutations and rebuilds derived artifacts.

### Flow

1. Write operations call `db.emit_compiler_event(CompilerEvent::*)`.
2. Events queue in an unbounded channel.
3. The compiler loop receives the first event, then drains all pending events within a 100ms debounce window.
4. Events are accumulated into a `CompilerPlan` describing which rebuilds are needed.
5. Compilers run in dependency order (status bitmaps → tag bitmaps → smart folders → sidebar → PTR overlay).
6. After completion, the `on_batch_done` callback (in `state.rs`) emits a `MutationImpact` with `compiler_batch_done: true`.

### CompilerPlan Dependency Rules

- `FileInserted/Deleted/StatusChanged` → rebuild status bitmaps + sidebar + all smart folders (status affects membership in every scope)
- `FileTagsChanged` → rebuild all smart folders + sidebar (tag predicates may depend on any tag)
- `TagChanged` → rebuild specific tag bitmap + sidebar (only the changed tag's bitmap)
- `TagGraphChanged` → rebuild tag graph + all smart folders + sidebar (parent changes cascade)
- `SmartFolderChanged` → rebuild specific smart folder + sidebar
- `FolderChanged` → rebuild sidebar only (folder bitmap is updated inline)

## Scope Cache

Grid paging uses a `ScopeSnapshot` cache to avoid rebuilding temp ID sets for consecutive page fetches in the same scope. Cache has a 30-second TTL and max 64 entries. Invalidated when membership-affecting bitmaps change.

## Key Files

- `core/src/sqlite/mod.rs` — `SqliteDatabase`, connection pool, manifest, scope cache
- `core/src/sqlite/bitmaps.rs` — `BitmapStore`, `BitmapKey`, serialization
- `core/src/sqlite/compilers.rs` — compiler loop, `CompilerEvent`, `CompilerPlan`
- `core/src/sqlite/schema.rs` — DDL, migrations, pragma application
- `core/src/sqlite/hash_index.rs` — bidirectional LRU hash↔file_id cache
