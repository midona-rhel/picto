# PBI-139: Export metadata to CSV

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has "Export to CSV" — export file metadata as spreadsheet.
2. Picto has no metadata export.

## Problem
Users cannot export their library metadata for analysis, reporting, or use in other tools.

## Scope
- Backend: CSV generation from file metadata queries
- `src/desktop/api.ts` — export command
- File save dialog

## Implementation
1. Export selected files (or all) metadata as CSV.
2. Columns: name, hash, file_type, dimensions, file_size, rating, tags (comma-separated), folders (comma-separated), date_created, date_imported, url.
3. File save dialog for output location.
4. UTF-8 encoding with BOM for Excel compatibility.

## Acceptance Criteria
1. Export generates valid CSV with all metadata columns.
2. Tags and folders properly formatted as comma-separated values.
3. File opens correctly in Excel/Numbers.

## Test Cases
1. Export 100 files — CSV has 100 data rows + header.
2. File with tags ["cat", "animal"] — CSV cell shows "cat, animal".
3. Open in Excel — columns align correctly.

## Risk
Low. Straightforward data serialization.
