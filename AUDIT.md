# Codebase Audit — Disconnected & Dead Code

Audit date: 2026-02-27

## Frontend Dead Code

### Files
- **`src/AppLegacy.tsx`** — Duplicate of `App.tsx`, not imported anywhere. Safe to delete.

### Unused Methods
- **`FolderController.getFolderFiles()`** (`src/controllers/folderController.ts:62-64`) — Defined but never called from any component. Note: `listFolders()` IS actively used by `folderPickerService.tsx`.

## Backend Stub Commands (28 total, kept intentionally)

These commands return static/empty responses. They exist so the frontend doesn't crash when calling them.

### Collections (6) — mapped to folders
`get_collections`, `create_collection`, `update_collection`, `delete_collection`, `get_collection_suggestions_for_review`, `scan_for_collections`

### Review Queue (4) — mapped to inbox (status=0)
`review_image_action`, `get_review_queue`, `get_review_item_image`, `get_review_item_thumbnail`

### Hydrus Integration (14) — external service stubs
All `hydrus_*` commands: `hydrus_set_duplicate_relationship`, `hydrus_smart_merge_duplicates`, `set_hydrus_client_api_config`, `set_hydrus_runtime_config`, `test_hydrus_client_api_connection`, `get_hydrus_client_api_config`, `get_hydrus_runtime_config`, `hydrus_client_api_proxy`, `hydrus_get_duplicate_groups`, `hydrus_get_file_data`, `hydrus_get_file_metadata`, `hydrus_get_files`, `hydrus_get_thumbnail`, `launch_hydrus_client`

### AI Tagger (8) — external service stubs
`initialize_ai_tagger`, `update_tagger_config`, `get_tagger_config`, `get_available_models`, `tag_image_by_hash`, `tag_image_from_bytes`, `tag_image_from_path`, `get_acceleration_backend`

### Other Stubs (2)
- `update_duplicate_config` / `get_duplicate_config` — duplicate config not yet in AppSettings schema

## Verified Active Code (previously suspected dead)

The following were initially suspected to be dead but are confirmed active:

- **`PtrSyncController`** — imported by `SidebarJobStatus.tsx`, drives PTR sync progress UI
- **`FolderController.listFolders()`** — used by `folderPickerService.tsx`
- **`SubscriptionController.runSubscription()`** — used by `FlowsWorking.tsx` and `SubscriptionsPanel.tsx`
- **`SubscriptionController.runSubscriptionQuery()`** — used by `SubscriptionsPanel.tsx`
- **`companion_get_namespace_values`** — used by `Collections.tsx` for namespace browsing
- **`companion_get_files_by_tag`** — used by `Collections.tsx` for filtered image loading
