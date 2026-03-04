# PBI-124: Folder cover image

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has "Set as Folder Cover" — any item can be set as the folder's cover thumbnail.
2. Picto shows no folder cover/thumbnail.

## Problem
Folders in the sidebar have no visual preview. Users cannot identify folders by a representative image.

## Scope
- Backend: `cover_id INTEGER` column on folders table (references file_id)
- `src/components/sidebar/FolderTree.tsx` — render cover thumbnail
- Context menu: "Set as Folder Cover"

## Implementation
1. Add `cover_id INTEGER` to folders table.
2. Context menu on files: "Set as Folder Cover" (when viewing a folder).
3. Sidebar renders small thumbnail next to folder name.
4. Fallback: first file in folder if no explicit cover set.

## Acceptance Criteria
1. Right-click file → Set as Folder Cover updates folder thumbnail.
2. Folder shows cover image in sidebar.
3. Removing cover falls back to first file.

## Test Cases
1. Set cover — sidebar folder shows image thumbnail.
2. Delete the cover file — folder falls back to next file.
3. Empty folder — no thumbnail shown.

## Risk
Low. Single FK + thumbnail rendering.
