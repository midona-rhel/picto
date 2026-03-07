# Grid Domain

## Purpose

The grid controller resolves paginated file queries for the main image grid. It handles scope resolution (which files are visible), filtering (tag/folder/status/smart folder predicates), sorting, and pagination.

## Status Semantics

- `0` = inbox (newly imported, unreviewed)
- `1` = active (reviewed, kept)
- `2` = trash (marked for deletion)

The default grid view shows status=1 (active) only. Inbox and trash are separate views.

## Scope Resolution

The grid controller uses bitmaps to determine which files are in scope. The scoping logic mirrors `selection_helpers::selection_bitmap_for_all_results` — both must produce identical results.

### Scoping Priority

1. **Smart folder** — if a smart folder predicate is present, it defines its own scope via `compile_predicate()`.
2. **Search tags** — tag search intersects with `AllActive` (inbox + active). Tags default to `All` match mode (AND).
3. **Folders** — folder scope intersects with `AllActive`. Folders default to `Any` match mode (OR).
4. **Status-only** — falls back to status-based views:
   - `"inbox"` → `Status(0)` bitmap
   - `"trash"` → `Status(2)` bitmap
   - `"untagged"` → `AllActive - Tagged`
   - `"uncategorized"` → SQL-based (files not in any folder)
   - `"recently_viewed"` → `AllActive` (filtered by view_count later)
   - Default → `Status(1)` (active only)

### Match Mode Defaults

- **Tags**: default to `All` (AND) — searching for multiple tags finds files with ALL of them.
- **Folders**: default to `Any` (OR) — viewing multiple folders shows files from ANY of them.

These defaults match user expectations: tag search narrows, folder viewing widens.

## Selection Modes

- `ExplicitHashes` — frontend provides specific file hashes (used for drag-select, shift-click).
- `AllResults` — backend resolves the current grid scope to a bitmap, then applies excluded_hashes.

## Key Files

- `core/src/grid_controller.rs` — grid page queries, pagination, scope resolution
- `core/src/selection_helpers.rs` — shared bitmap resolution for selection operations
- `core/src/dispatch/typed/grid.rs` — typed command handlers for grid queries
