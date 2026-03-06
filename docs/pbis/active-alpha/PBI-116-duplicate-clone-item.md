# PBI-116: Duplicate/clone item

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Cmd+D to duplicate/clone items within the library.
2. Picto has no duplicate file operation.

## Problem
Users cannot create copies of files within the library for variant exploration.

## Scope
- Backend: `core/src/dispatch/files_lifecycle.rs` — duplicate command
- `core/src/sqlite/files.rs` — clone file record with new hash/ID
- `core/src/import.rs` — copy blob to new location
- `src/lib/shortcuts.ts` — Cmd+D shortcut
- `src/desktop/api.ts` — `api.file.duplicate()` method

## Implementation
1. Backend command `duplicate_files`: for each selected file, copy blob to new hash, create new DB record copying all metadata (tags, folders, notes, rating).
2. New file gets new file_id and hash (content-addressed copy or reference copy).
3. Frontend: Cmd+D triggers duplicate on selection.
4. Toast notification: "Duplicated N items".

## Acceptance Criteria
1. Cmd+D creates a copy of selected files.
2. Duplicated files retain tags, folders, rating, and notes.
3. Duplicated files appear in the same folder(s) as originals.

## Test Cases
1. Select file, Cmd+D — new file appears with same tags.
2. Duplicate multiple selections — all copied.
3. Edit duplicate — original unchanged.

## Risk
Low. Straightforward blob copy + DB record duplication.
