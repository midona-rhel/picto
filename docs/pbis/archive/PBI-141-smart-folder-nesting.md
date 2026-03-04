# PBI-141: Smart folder nesting

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has smart folder groups/nesting — smart folders can be organized into parent groups.
2. Picto has flat smart folder list only.

## Problem
Users with many smart folders cannot organize them into logical groups.

## Scope
- Backend: `parent_id` column on smart_folders table
- `src/components/sidebar/SmartFolderList.tsx` — tree rendering
- `src/components/smart-folders/SmartFolderModal.tsx` — parent selection

## Implementation
1. Add `parent_id INTEGER` to smart_folders table (self-referencing FK).
2. Smart folder groups: create a "group" smart folder that acts as a container.
3. Drag smart folders into groups in sidebar.
4. Collapsible groups in sidebar.
5. Smart folder modal: optional parent group selector.

## Acceptance Criteria
1. Can create smart folder groups.
2. Can drag smart folders into groups.
3. Groups are collapsible in sidebar.

## Test Cases
1. Create group "Photo Filters", drag smart folders into it — nested display.
2. Collapse group — children hidden.
3. Delete group — children move to root level.

## Risk
Low. Self-referencing FK + tree rendering (already done for folders).
