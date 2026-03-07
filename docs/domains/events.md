# Events Domain

## Purpose

The events system notifies the frontend about backend state changes. The primary mechanism is `MutationImpact` — a structured description of what changed and what the frontend should refresh.

## Event Lifecycle

1. Dispatch handler executes a mutation (tag change, file import, status update, etc.)
2. Handler constructs a `MutationImpact` using a named preset constructor
3. Handler calls `emit_mutation(command_name, impact)`
4. `emit_mutation` wraps the impact in a `MutationReceipt` with sequence number and timestamp
5. Receipt is serialized and sent via the global event callback to the frontend
6. Frontend `eventBridge` processes the receipt and triggers targeted invalidation

## MutationImpact Presets

Each preset encodes domain-specific invalidation rules. The rationale for each:

| Preset | Domains | Invalidation | Why |
|--------|---------|-------------|-----|
| `file_lifecycle` | Files, Sidebar, SmartFolders | sidebar + grid_all + selection + counts | New/deleted/status-changed files affect all aggregate views |
| `file_metadata` | Files | metadata_hashes(hash) | Rating/notes/URL changes only affect the detail view for that file |
| `file_tags` | Tags, Files | metadata_hashes(hash) | Tag change on one file updates its detail view; compiler handles bitmap updates separately |
| `batch_tags` | Tags, Files | grid_all + selection | Batch tag operations affect grid sort/filter and selection summary |
| `sidebar` | (param), Sidebar | sidebar_tree | Folder/smart folder/subscription CRUD changes sidebar structure |
| `file_status_change` | Files, Sidebar, Folders, SmartFolders, Selection | sidebar + selection + counts | Status transitions affect folder membership, smart folder results, and counts |
| `folder_file_change` | Folders, Files, Selection, Sidebar | sidebar + grid(folder:N) + selection | Adding/removing from a folder only invalidates that folder's grid |
| `tag_structure_change` | Tags, Sidebar, SmartFolders | sidebar + grid_all + selection | Merge/delete/normalize affects tag bitmaps, smart folder predicates, and sidebar counts |
| `folder_item_reorder` | Folders | grid(folder:N) | Reordering only affects visual order within the specific folder |
| `all_domains_change` | Files, Folders, Tags, Sidebar, SmartFolders | sidebar + selection + counts | Subscription import with collections touches everything |
| `selection_batch_tags` | Tags, Files, Selection | selection + grid_all | Selection-scoped tag operations |
| `collection_update` | Folders, Sidebar | folder_ids + grid_all + selection | Collection metadata change |
| `domain_only` | (param) | none | Minimal mutations that don't need UI refresh (subscription CRUD, duplicate resolution) |
| `selection_metadata` | Files | selection_summary | Selection-scoped metadata (notes, URLs) — only selection panel needs refresh |
| `selection_metadata_grid` | Files | selection + grid_all | Selection-scoped metadata that affects grid (rating changes sort order) |
| `view_prefs_change` | ViewPrefs | view_prefs | Layout/sort/tile size preferences |

## Sidebar Counts

`sidebar_counts_from_bitmaps(db)` computes O(1) sidebar counts from bitmaps:
- `all_images` = `Status(1)` count (active only — not inbox)
- `inbox` = `Status(0)` count
- `trash` = `Status(2)` count

Only presets that affect file counts include sidebar counts (file_lifecycle, file_status_change, all_domains_change).

## Other Event Types

Beyond `MutationReceipt`, domain-specific lifecycle events exist for long-running operations:
- Subscription: `subscription-started`, `subscription-progress`, `subscription-finished`
- Flow: `flow-started`, `flow-progress`, `flow-finished`
- PTR sync: `ptr-sync-started`, `ptr-sync-progress`, `ptr-sync-finished`
- PTR bootstrap: `ptr-bootstrap-started/progress/finished/failed`
- System: `library-closed`, `zoom-factor-changed`, `file-imported`
- Runtime tasks: `runtime/task_upserted`, `runtime/task_removed`

## Key Files

- `core/src/events.rs` — event emission, `MutationImpact` presets, event name constants
- `core/src/runtime_contract/mutation.rs` — `MutationReceipt`, `MutationFacts`, `DerivedInvalidation`
- `src/stores/eventBridge.ts` — frontend event ingestion
