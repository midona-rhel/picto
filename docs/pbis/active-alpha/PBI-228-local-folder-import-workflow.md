# PBI-228: Local folder import workflow

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user asked "how to use a folder I have locally" and was told "Importing folders is hard, drag and drop works tho."
2. Drag-and-drop import of individual files works, but there is no dedicated workflow for importing an entire local folder (preserving its structure as Picto folders).
3. Eagle supports folder import with structure preservation via File > Import > Import Folder.

## Problem
Users with existing media collections on disk have no structured way to import a local folder tree into Picto. Drag-and-drop works for loose files but does not preserve folder hierarchy or handle large folder imports gracefully.

## Scope
- Import dialog or menu action for selecting a local folder
- Recursive scan of selected folder
- Option to preserve folder structure as Picto folders or flatten into a single destination
- Progress indicator for large imports

## Implementation
1. Add "Import Folder" action accessible from the sidebar or a menu.
2. Open a native folder picker dialog.
3. Recursively scan the selected folder for supported media files.
4. Show a preview/confirmation step: file count, total size, and structure preservation toggle.
5. If preserving structure: create matching Picto folder hierarchy and assign files accordingly.
6. If flattening: import all files into the current folder or library root.
7. Run import as a background task with progress reporting through the existing task/event system.
8. Handle duplicates according to existing duplicate detection settings.

## Acceptance Criteria
1. User can select a local folder and import all supported media from it.
2. Folder structure can optionally be preserved as Picto folders.
3. Large imports (1000+ files) run in background with visible progress.
4. Duplicate files are detected and handled per user settings.
5. Unsupported file types are skipped with a summary shown after import.

## Test Cases
1. Import folder with 50 images, preserve structure — Picto folders mirror disk structure.
2. Import folder with 50 images, flatten — all files in one Picto folder.
3. Import folder with 2000 files — progress bar visible, completes without UI freeze.
4. Import folder containing duplicates of existing library items — duplicates handled per settings.
5. Import folder with mixed media and unsupported files — unsupported files skipped, summary shown.

## Risk
Medium. Recursive folder scanning and structure preservation require mapping between filesystem paths and Picto's folder model. Large imports need to be non-blocking.
