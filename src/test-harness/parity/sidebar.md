# Shell / Sidebar Parity Checklist

Reference: PBI-591 (shell/sidebar rebuild)
Fixtures: `src/test-harness/fixtures/sidebar.ts`
Last evaluated: 2026-03-24

## Structure

- [x] Section nodes (system:library, section:folders, section:smart_folders) render as non-selectable headers
- [x] System scopes are children of system:library, in order: All Active, Inbox, Uncategorized, Untagged, Recently Viewed, Duplicates, Trash
- [x] System scope IDs match live contract: system:active (NOT system:all), system:inbox, system:uncategorized, system:untagged, system:recent_viewed, system:duplicates, system:trash
- [x] Folders are children of section:folders (top-level) or folder:{parentId} (nested)
- [x] Deeply nested folders (3+ levels) render with correct indentation
- [x] Smart folders are children of section:smart_folders (top-level) or smart:{parentId} (nested)
- [x] Smart folder IDs use smart:{id} format (NOT smart_folder:{id})
- [x] Empty folders (count=0) still appear in the tree
- [x] Folder icons and colors render when present
- [x] Smart folder icons and colors render when present

## Counts

- [x] Each node displays its count badge
- [x] Count of 0 shows "0" (not hidden)
- [x] Count of null shows no badge (not "0")
- [x] Stale freshness: count renders dimmed (countStale prop)
- [x] Rebuilding freshness: count renders dimmed (same as stale)
- [x] Exact freshness: count renders normally

## Interactions

- [x] Clicking a system scope updates activeNodeId (grid integration is PBI-592)
- [x] Clicking a folder updates activeNodeId
- [x] Clicking a smart folder updates activeNodeId
- [x] Right-click on folder opens context menu with real actions (create, rename, delete, color) and honest TODOs
- [x] Right-click on smart folder opens context menu (delete working, rename/color/icon are TODOs)
- [x] Folder expand/collapse state persists across navigation and sessions (localStorage)
- [n/a] Drag-drop reordering of folders — deferred, follow-up sidebar interaction PBI
- [n/a] Drag-drop of entities onto a folder — requires grid (PBI-592+)
- [ ] expanded_by_default=true folders start expanded on first load — sections respect it, individual folders do not

## Shell layout

- [x] Sidebar occupies left panel with fixed width (--sidebar-width: 290px)
- [x] Sidebar collapse/expand toggle works (visible button in sidebar header + restore button in main area when collapsed + Cmd+\, persisted to localStorage)
- [x] Main content area fills remaining space
- [n/a] Sidebar resize via drag handle — explicitly not part of PBI-591 activation bar. Resize is a follow-up polish item, not a core shell behavior.
- [n/a] Titlebar with window controls — Electron manages the native titlebar. The rebuilt shell does not compose its own titlebar. This is not a blocker for PBI-591 activation.

## Edge cases

- [x] Empty library: only system scopes visible, all counts zero
- [x] Sidebar tree updates when state_changed event arrives with sidebar domain (sidebarSettle.ts)
- [x] Smart folder with complex predicate in meta: does not crash rendering
- [ ] Library with 1000+ folders: not tested, non-blocking unless perf issue found

## Legacy behavior to preserve (not legacy architecture)

- [x] Folder tree matches reference application-style visual weight (not Finder-style)
- [x] Selected node has distinct highlight (active class with surface-active bg)
- [x] Hover state on nodes is visible but subtle (surface-hover bg via ::before)
- [x] System scope icons — curated frontend icon map (IconPhoto, IconInbox, etc.)

## Accepted differences from legacy

- System scopes show "All" instead of "All Active" — intentional product decision
- Secondary scopes (Recently Viewed, Duplicates) shown below a separator — intentional layout change
- No LibrarySwitcher — deferred, not core sidebar interaction
- No SidebarJobStatus — belongs to subscriptions surface (PBI-595)
- Smart folder rename/color/icon present in UI but are TODOs — backend requires full struct update
- No sidebar resize — not part of activation bar
- No composed titlebar — Electron native titlebar is sufficient

## Activation summary

**Passing: 24 of 26 evaluated items** (2 remaining are non-blocking)
**Not applicable: 4 items** (explicitly deferred or out of scope)

Remaining non-blocking items:
1. expanded_by_default on individual folder nodes — minor, sections already respect it
2. 1000+ folder perf — test when needed

**PBI-591 activation bar is met.**
