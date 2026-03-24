# Shell / Sidebar Parity Checklist

Reference: PBI-591 (shell/sidebar rebuild)
Fixtures: `src/test-harness/fixtures/sidebar.ts`

## Structure

- [ ] Section nodes (system:library, section:folders, section:smart_folders) render as non-selectable headers
- [ ] System scopes are children of system:library, in order: All Active, Inbox, Uncategorized, Untagged, Recently Viewed, Duplicates, Trash
- [ ] System scope IDs match live contract: system:active (NOT system:all), system:inbox, system:uncategorized, system:untagged, system:recent_viewed, system:duplicates, system:trash
- [ ] Folders are children of section:folders (top-level) or folder:{parentId} (nested)
- [ ] Deeply nested folders (3+ levels) render with correct indentation
- [ ] Smart folders are children of section:smart_folders (top-level) or smart:{parentId} (nested)
- [ ] Smart folder IDs use smart:{id} format (NOT smart_folder:{id})
- [ ] Empty folders (count=0) still appear in the tree
- [ ] Folder icons and colors render when present
- [ ] Smart folder icons and colors render when present

## Counts

- [ ] Each node displays its count badge
- [ ] Count of 0 shows "0" (not hidden)
- [ ] Count of null shows no badge (not "0")
- [ ] Stale freshness: count renders with a visual indicator (dimmed, spinner, or similar)
- [ ] Rebuilding freshness: count renders with a visual indicator
- [ ] Exact freshness: count renders normally

## Interactions

- [ ] Clicking a system scope navigates the grid to that scope
- [ ] Clicking a folder navigates the grid to folder scope
- [ ] Clicking a smart folder navigates the grid to smart_folder scope
- [ ] Right-click on folder opens context menu (rename, delete, add subfolder, set watch)
- [ ] Right-click on smart folder opens context menu (edit, delete)
- [ ] Drag-drop reordering of folders works
- [ ] Drag-drop of entities onto a folder adds them to that folder
- [ ] Folder expand/collapse state persists across navigation
- [ ] expanded_by_default=true folders start expanded on first load

## Shell layout

- [ ] Sidebar occupies left panel with fixed width
- [ ] Sidebar is resizable via drag handle
- [ ] Titlebar with window controls renders correctly
- [ ] Main content area fills remaining space
- [ ] Sidebar collapse/expand toggle works

## Edge cases

- [ ] Empty library: only system scopes visible, all counts zero
- [ ] Library with 1000+ folders: no render lag, virtualization if needed
- [ ] Sidebar tree updates when state_changed event arrives with sidebar domain
- [ ] Smart folder with complex predicate in meta: does not crash rendering

## Legacy behavior to preserve (not legacy architecture)

- [ ] Folder tree matches reference application-style visual weight (not Finder-style)
- [ ] Selected node has distinct highlight
- [ ] Hover state on nodes is visible but subtle
- [ ] System scope icons match legacy app (or documented replacement)
