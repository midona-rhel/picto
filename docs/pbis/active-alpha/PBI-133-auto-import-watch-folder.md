# PBI-133: Auto-import watch folder

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has auto-import: file system watcher monitors a folder, auto-imports any new files.
2. Picto has no auto-import/watch folder.

## Problem
Users must manually import files. No way to auto-ingest from a download folder or staging area.

## Scope
- Backend: file system watcher (notify crate) on configured folder
- Settings: watch folder path, enable/disable toggle
- `src/components/settings/GeneralPanel.tsx` — auto-import settings

## Implementation
1. Use `notify` crate to watch a configured directory for new files.
2. Debounce detection (500ms) and wait for file write completion.
3. Auto-import detected files into library.
4. Settings panel: select folder, enable/disable toggle.
5. Notification on auto-import (toast).
6. Configurable: auto-tag imported files, target folder.

## Acceptance Criteria
1. Configure watch folder in settings.
2. Drop file into watched folder — auto-imported into library.
3. Notification shown on import.
4. Disable toggle stops watching.

## Test Cases
1. Enable watch on ~/Downloads — copy image there → appears in library.
2. Disable — copy another image → not imported.
3. Large file: wait for write completion before importing.

## Risk
Medium. File system watchers can be platform-specific. Debouncing and completion detection needed.
