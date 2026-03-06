# PBI-231: Windows platform — collection creation and reorder fixes

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user on Windows reported: "creating collection seems to do nothing" — collection was created but never appeared in the sidebar/grid.
2. Confirmed: "also found that u cant rearrange stuff in windows — also a bug."
3. Workaround discovered: reinstall, clear AppData settings, create new library. Suggests stale or corrupted local state on Windows.
4. Collection creation and item reordering work correctly on macOS.

## Problem
Two Windows-specific issues with collections:
1. **Collection creation invisible**: collections are created in the backend but do not appear in the UI until the user clears AppData and recreates their library.
2. **Item reordering broken**: drag-to-reorder within a collection does not persist or visually update on Windows.

## Scope
- Collection creation flow — investigate why the UI doesn't refresh/navigate to the new collection on Windows
- Item reorder (drag-and-drop within collection) — investigate platform-specific DnD or event handling differences
- Windows AppData / local state handling — investigate corruption scenarios

## Implementation
1. **Collection visibility**: after creating a collection, force a sidebar/navigation refresh and navigate to the new collection. Investigate if the issue is a missing IPC event, a stale cache, or a rendering race condition specific to Windows.
2. **Reorder persistence**: check if the drag-and-drop reorder events fire correctly on Windows. Investigate @dnd-kit behavior differences on Windows (pointer event handling, drag thresholds). Ensure the reorder mutation is called and the new order persists to the database.
3. **State corruption guard**: add integrity checks on startup for the local navigation/sidebar state. If the state references entities that don't exist in the database, rebuild it.

## Acceptance Criteria
1. Create a collection on Windows — it appears immediately in the sidebar without manual refresh.
2. Reorder items within a collection on Windows — new order persists after navigation and restart.
3. No AppData clearing or library recreation required for normal operation.
4. Behavior matches macOS on all collection operations.

## Test Cases
1. Windows: select 3 images, right-click > Create Collection — collection appears in sidebar.
2. Windows: open collection, drag item from position 3 to position 1 — order updates and persists.
3. Windows: create collection, close app, reopen — collection still visible.
4. Windows: fresh install, create library, create collection — works on first attempt.
5. macOS: same operations — no regression.

## Risk
Medium. Windows-specific issues may stem from Electron IPC timing, filesystem path handling (backslashes), or pointer event differences in @dnd-kit. Debugging requires a Windows test environment.
