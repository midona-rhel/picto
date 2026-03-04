# PBI-138: Replace file (keep metadata)

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has "Replace with File" — swap file contents while keeping all metadata (tags, folders, rating, notes).
2. Picto has no file replacement.

## Problem
When users update a file (e.g. edited version), they must delete old and re-import, losing all tags and folder assignments.

## Scope
- Backend: `core/src/dispatch/files_lifecycle.rs` — replace file command
- `core/src/import.rs` — re-import pipeline (new hash, keep metadata)
- Context menu entry

## Implementation
1. "Replace with File..." context menu: file picker for replacement.
2. Backend: compute new hash, copy to blob store, update file record (hash, size, dimensions, mime), regenerate thumbnail.
3. Preserve: tags, folders, rating, notes, creation date.
4. Update: hash, file_size, dimensions, modified_date.
5. Confirmation dialog: "This will permanently replace the file. Continue?"

## Acceptance Criteria
1. Replace file updates content while keeping all metadata.
2. Thumbnail regenerated from new content.
3. File hash updated in blob store.
4. Tags, folders, rating all preserved.

## Test Cases
1. Replace PNG with updated version — tags and folders unchanged.
2. Thumbnail reflects new image.
3. File size/dimensions update to new file's values.

## Risk
Low-Medium. Need to handle hash change in blob store + hash index.
