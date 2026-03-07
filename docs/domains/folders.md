# Folders Domain

## Purpose

Folders are user-created containers for organizing files. Unlike tags (which are metadata), folders represent explicit file grouping with manual ordering support. Collections are a folder subtype that support manual member ordering via gap-based ranking.

## Lifecycle

1. **Create** — user creates folder, sidebar node inserted immediately for fast UI feedback.
2. **Add files** — files are added to folders via `add_entity_to_folder`. Compiler updates the `Folder(id)` bitmap.
3. **Ordering** — collections support manual ordering via gap-based ranking (float sort values with gaps for insertion).
4. **Delete** — folder deletion removes the folder row and all `folder_entity` membership rows.

## Sidebar Projection

Folders appear in the sidebar tree under `section:folders`. Each folder node includes:
- `node_id`: `folder:{id}`
- `count`: number of files in the folder
- `meta_json`: contains `folder_id`, `auto_tags`, `is_collection`, `parent_id`

Sidebar nodes are rebuilt by the compiler when folder membership or structure changes.

## Gap-Based Ranking

Collections use float-valued sort keys with large gaps (e.g., 1000.0, 2000.0) between items. Inserting between two items picks the midpoint. When gaps become too small, a full rebalance redistributes sort values evenly.

## Key Invariants

- `Folder(id)` bitmap must be updated when files are added/removed from a folder.
- Folder membership is independent of file status — trash files can still be in folders.
- The `uncategorized` view shows files not in ANY folder (computed via SQL, not bitmaps).

## Key Files

- `core/src/sqlite/folders.rs` — folder CRUD, gap-based ranking, entity membership
- `core/src/folder_controller.rs` — orchestration
- `core/src/dispatch/typed/folders.rs` — typed command handlers
