# PBI-246: Add-to-folder modal with tree view

## Priority
P2

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. "Add to folder" no longer opens a nested submenu. It now opens the shared folder picker portal from [src/shared/services/FolderPickerPortal.tsx](./src/shared/services/FolderPickerPortal.tsx).
2. The picker already supports search, keyboard navigation, multi-selection use, and a stable floating panel UI instead of fragile submenu hover behavior.
3. However, the picker is still a flat alphabetized list (`flatFolders`) rather than a hierarchical tree matching the sidebar.
4. There is still no expand/collapse navigation for nested folders, so the core tree-view requirement from this PBI remains open.

## Problem
The submenu problem is solved, but the folder picker still does not expose actual hierarchy. For large nested libraries, a flat searchable list is better than a submenu, but still weaker than a real tree view.

## Desired behavior
1. Clicking "Add to folder" in the context menu opens a **modal dialog** (not a submenu).
2. The modal contains a **tree view** matching the sidebar folder hierarchy — expandable nodes, nested subfolders.
3. The user clicks a folder in the tree to select it, then confirms (or single-click-to-confirm for speed).
4. Works for both single image and multi-selection — all selected images are added to the chosen folder.

## Scope
- Context menu "Add to folder" action — change from submenu to modal trigger
- New modal component: `AddToFolderModal` (or reuse a shared modal pattern)
- Tree view component — reuse or adapt the sidebar tree view
- Backend: reuse existing add-to-folder mutation (already supports batch)

## Implementation
1. Replace the "Add to folder" submenu in the context menu with a single action that opens a modal.
2. The modal renders a tree view of all folders, reusing the sidebar's tree data and expand/collapse logic.
3. Clicking a folder in the tree selects it (highlight). A confirm button (or double-click) executes the add.
4. Pass the current selection (single hash or multi-selection hashes) to the modal.
5. On confirm, call the existing add-to-folder backend command with all selected entity IDs and the target folder ID.
6. Close the modal and show a brief toast: "Added N items to [folder name]".

## Acceptance Criteria
1. "Add to folder" opens a modal or dedicated picker surface, not a context menu submenu.
2. Modal shows a tree view matching the sidebar folder hierarchy.
3. Folders can be expanded/collapsed to navigate nested structure.
4. Single image: added to selected folder on confirm.
5. Multi-selection: all selected images added to selected folder on confirm.
6. Picker is keyboard-navigable (arrow keys for tree/list, Enter to confirm, Escape to cancel).

## Test Cases
1. Right-click single image → Add to folder → modal opens with tree → select folder → image added.
2. Select 10 images → right-click → Add to folder → modal → select folder → all 10 added.
3. Expand nested folder in tree → select subfolder → images added to subfolder.
4. Press Escape → modal closes, no action taken.
5. Folder tree matches sidebar (same folders, same hierarchy).

## Risk
Low-medium. Tree view component may need to be extracted from the sidebar into a shared component. Modal pattern should follow existing Mantine modal conventions.
