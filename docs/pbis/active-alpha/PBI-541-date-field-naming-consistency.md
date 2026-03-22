# PBI-541: Date field naming consistency

## Priority
P2

## Problem
Date fields use inconsistent names across the backend and frontend, making the data model confusing:

- `file.imported_at` — actually means "date added to library" (always `Utc::now()`)
- `media_entity.created_at` — sometimes means "original content creation date" (from gallery-dl metadata), sometimes means "entity creation time" (for collections)
- `options.created_at` — the metadata date from the source site, which gets stored as `entity_created_at` on `NewFile`, then becomes `media_entity.created_at`
- `date_added` — the frontend display name, maps to `COALESCE(f.imported_at, me.created_at)` in SQL
- Grid sort uses `date_added` which resolves differently for singles vs collections

The names don't describe what they actually hold:
- "imported_at" is always now — it's really "date_added"
- "created_at" on entities holds the source content date — it's really "date_created" or "origin_date"
- "entity_created_at" on NewFile is a pass-through that should just be called "origin_date"

## Scope
- `core/src/sqlite/files.rs` — `NewFile.imported_at`, `NewFile.entity_created_at`, `FileMetadataSlim.imported_at`
- `core/src/import/db.rs` — `ImportOptions.created_at`
- `core/src/folders/collections_db.rs` — `sync_collection_aggregate_metadata` date queries
- `core/src/sqlite/schema/ddl.rs` — column names in `file` and `media_entity` tables
- `src/shared/types/api/core.ts` — `EntitySlim.date_added`
- Grid sort expressions in `entity_sort_expr()`
- `ENTITY_SLIM_SELECT` SQL

## Proposed naming

| Current name | Actual meaning | Proposed name |
|---|---|---|
| `file.imported_at` | When the file was added to the library | `date_added` |
| `media_entity.created_at` | Original content creation date (from source) | `date_created` |
| `NewFile.entity_created_at` | Pass-through of source date | `date_created` |
| `ImportOptions.created_at` | Source site content date | `date_created` |

Display:
- **Date added** = when the item entered the library (`file.imported_at` / now)
- **Date created** = when the content was originally created at the source (`media_entity.created_at` / gallery-dl metadata)

## Implementation
1. Rename DB columns via migration (V36): `file.imported_at → date_added`, `media_entity.created_at → date_created`
2. Update all Rust structs, SQL queries, sort expressions
3. Update frontend types
4. Keep backward-compatible aliases in the grid query layer during transition

## Acceptance Criteria
1. All date fields use consistent, self-documenting names
2. Inspector shows both "Date added" and "Date created" with correct values
3. Sort by "Date added" uses library import time; sort by "Date created" uses source content time
4. No data loss during migration
