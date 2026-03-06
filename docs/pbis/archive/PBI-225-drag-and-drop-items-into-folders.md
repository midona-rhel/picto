# PBI-225: Drag and drop items into folders

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Eagle supports dragging items from the grid directly onto folders in the sidebar to add them.
2. Picto has @dnd-kit infrastructure and uses it for folder reordering, but does not support dragging media items from the grid into sidebar folders.

## Problem
Users expect to organize media by dragging items from the grid onto folders in the sidebar. Currently the only way to add items to a folder is through context menus or the inspector, which is slower for bulk organization workflows.

## Scope
- `src/components/image-grid/` — make grid items draggable with entity payload
- `src/components/sidebar/` — make folder tree nodes droppable targets
- Backend: reuse existing add-to-folder mutation
- Visual feedback: drop highlight on valid folder targets, rejection indicator for invalid drops

## Implementation
1. Wrap grid media items as `<Draggable>` sources carrying entity IDs (respect current multi-selection so dragging one selected item drags the whole selection).
2. Mark sidebar folder nodes as `<Droppable>` targets using @dnd-kit.
3. On drop, call the existing add-to-folder backend command with the dragged entity IDs and target folder ID.
4. Show a ghost preview with item count badge during drag.
5. Highlight the hovered folder with an accent border/background while dragging over it.
6. If the item is already in the target folder, show a subtle "already here" indicator instead of duplicating.

## Acceptance Criteria
1. Single item can be dragged from grid onto a sidebar folder to add it.
2. Multi-selected items can be dragged as a group onto a folder, all are added.
3. Valid drop targets highlight on drag-over; non-folder areas do not accept drops.
4. Drag preview shows item thumbnail and count badge for multi-selection.
5. Items already in the target folder are handled gracefully (no duplicates, no error).

## Test Cases
1. Drag one image onto a folder — image appears in that folder.
2. Select 5 images, drag onto folder — all 5 added.
3. Drag over non-folder sidebar area — no drop highlight, drop is rejected.
4. Drag item already in target folder — no duplicate entry created.
5. Drag onto a nested subfolder — item added to the correct subfolder.

## Risk
Medium. @dnd-kit is already integrated for folder reordering, but combining draggable grid items with droppable sidebar targets across different component trees may need a shared DndContext at a higher level.
