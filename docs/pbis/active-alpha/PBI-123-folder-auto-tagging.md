# PBI-123: Folder auto-tagging

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has "Set Auto-Tag" on folders — automatically adds specified tags to any item placed in the folder.
2. Picto has no auto-tagging on folder assignment.

## Problem
Users must manually tag files after organizing into folders, even when folder membership implies certain tags.

## Scope
- Backend: `auto_tags TEXT` column on folders table (JSON array of tag strings)
- `core/src/dispatch/folders.rs` — apply auto-tags when files added to folder
- Context menu: "Set Auto-Tag..." on folders

## Implementation
1. Add `auto_tags TEXT` column to folders table (stores JSON array).
2. Dialog: tag input for setting folder auto-tags.
3. When files are added to a folder (assign, drag-drop, import), auto-apply the folder's tags.
4. Context menu on folder: "Set Auto-Tag...".
5. Show auto-tags indicator on folder in sidebar.

## Acceptance Criteria
1. Can set auto-tags on a folder via context menu.
2. Adding a file to the folder automatically applies those tags.
3. Auto-tags indicator visible on folder.

## Test Cases
1. Set auto-tags ["landscape", "nature"] on folder → drag file in → file gets both tags.
2. Remove auto-tag from folder → future additions no longer get that tag.
3. Existing files in folder not retroactively tagged (only new additions).

## Risk
Low. Hook into existing folder assignment logic.
