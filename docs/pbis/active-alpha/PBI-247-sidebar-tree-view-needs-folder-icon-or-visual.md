# PBI-247: Sidebar folder tree view needs tree/expand visual indicators

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. The sidebar folder tree currently does not display expand/collapse chevrons or tree-line visual indicators for nested items.
2. It is unclear from the UI which folders have children and which are leaf nodes.
3. Users cannot tell at a glance how the folder hierarchy is structured.

## Problem
The sidebar folder tree view lacks visual tree affordances — no chevrons, no indentation guides, no tree lines. Users cannot distinguish between folders with subfolders and leaf folders without clicking each one.

## Scope
- `src/components/sidebar/` — folder tree rendering

## Implementation
1. Add expand/collapse chevron icons to folders that have children.
2. Show indentation guides (tree lines or stepped indentation) to visually communicate nesting depth.
3. Leaf folders (no children) show no chevron or a different icon.
4. Chevron rotates on expand/collapse (standard tree affordance).

## Acceptance Criteria
1. Folders with children show an expand/collapse chevron.
2. Nested folders are visually indented with guides or stepped padding.
3. Leaf folders are visually distinct from parent folders.
4. Expand/collapse is smooth (chevron rotation animation).

## Test Cases
1. Folder with 3 subfolders — chevron visible, click to expand shows children indented.
2. Leaf folder — no chevron, no expand action.
3. Deeply nested folders (3+ levels) — indentation is clear and readable.

## Risk
Low. Standard tree view UI pattern.
