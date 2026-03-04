# PBI-110: Uncategorized view (files not in any folder)

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Cmd+2 for Uncategorized — shows files not assigned to any folder.
2. Picto has no equivalent view. Cmd+2 maps to Inbox which is a status filter, not folder membership.

## Problem
Users cannot easily find files that haven't been organized into folders.

## Scope
- Backend: query for files with no folder membership
- `src/stores/navigationStore.ts` — add uncategorized filter
- `src/components/sidebar/Sidebar.tsx` — sidebar item with count

## Implementation
1. Add backend query: files where file_id not in any folder_membership row.
2. Add "Uncategorized" sidebar item with count.
3. Wire to navigation store as a status filter.

## Acceptance Criteria
1. Uncategorized view shows only files not in any folder.
2. Count in sidebar updates when files are added/removed from folders.

## Test Cases
1. Import file without assigning to folder → appears in Uncategorized.
2. Add that file to a folder → disappears from Uncategorized.

## Risk
Low. Straightforward NOT IN query.
