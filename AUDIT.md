# Codebase Audit — Disconnected & Dead Code

Audit date: 2026-02-27
Last updated: 2026-03-07

## Frontend Dead Code

No known dead code remaining. Previously identified items have been resolved:

- `src/AppLegacy.tsx` — deleted
- `FolderController.getFolderFiles()` — now actively used by `FolderTree.tsx`

## Backend Stub Commands

All 28 legacy stub commands identified on 2026-02-27 have been removed during the typed dispatch migration (PBI-234/326):

- **Collections** (6) — `get_collections`, `create_collection`, `update_collection`, `delete_collection` are now real typed commands. `get_collection_suggestions_for_review` and `scan_for_collections` were deleted (unused).
- **Review Queue** (4) — deleted. Functionality covered by `update_file_status` and grid `system:inbox` scope.
- **Hydrus Integration** (14) — deleted.
- **AI Tagger** (8) — deleted.
- **Duplicate config** (2) — deleted.

## Verified Active Code (previously suspected dead)

The following were initially suspected to be dead but are confirmed active:

- **`PtrSyncController`** — imported by `SidebarJobStatus.tsx`, drives PTR sync progress UI
- **`FolderController.listFolders()`** — used by `folderPickerService.tsx`
- **`SubscriptionController.runSubscription()`** — used by `FlowsWorking.tsx` and `SubscriptionsPanel.tsx`
- **`SubscriptionController.runSubscriptionQuery()`** — used by `SubscriptionsPanel.tsx`
- **`companion_get_namespace_values`** — used by `Collections.tsx` for namespace browsing
- **`companion_get_files_by_tag`** — used by `Collections.tsx` for filtered image loading
