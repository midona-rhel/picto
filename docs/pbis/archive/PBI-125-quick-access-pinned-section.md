# PBI-125: Quick Access pinned section

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Quick Access section at top of sidebar for pinned folders/smart folders.
2. Picto has no pinning or quick access.

## Problem
Users cannot pin frequently-used folders for quick access. Must scroll sidebar tree to find them.

## Scope
- Backend: `pinned BOOLEAN` column on folders/smart_folders tables
- `src/components/sidebar/Sidebar.tsx` — Quick Access section above folder tree
- Context menu: "Add to Quick Access" / "Remove from Quick Access"

## Implementation
1. Add `pinned INTEGER DEFAULT 0` to folders and smart_folders tables.
2. Quick Access section in sidebar above folder tree, showing pinned items.
3. Context menu: "Add to Quick Access" on any folder or smart folder.
4. Drag reorder within Quick Access section.
5. Collapsible section with item counts.

## Acceptance Criteria
1. Right-click folder → Add to Quick Access pins it to top section.
2. Quick Access section shows all pinned items.
3. Remove from Quick Access unpins the item.
4. Pinned state persists across sessions.

## Test Cases
1. Pin 3 folders — all appear in Quick Access.
2. Unpin one — only 2 remain.
3. Click pinned folder — navigates to that folder's content.

## Risk
Low. Boolean flag + sidebar section.
