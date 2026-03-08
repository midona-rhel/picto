# PBI-251: Import progress indicator

## Priority
P1

## Audit Status (2026-03-08)
Status: **Not Implemented**

Evidence:
1. Manual import in [src/features/grid/ImageGrid.tsx](./src/features/grid/ImageGrid.tsx) still only shows a completion toast after `api.import.files(...)` resolves.
2. There is no renderer event carrying per-file manual import progress, and `import_files` in [core/src/dispatch/typed/media_lifecycle.rs](./core/src/dispatch/typed/media_lifecycle.rs) still returns only the final batch result.
3. The only live per-file import event currently exposed to the renderer is `file-imported`, emitted from subscription sync in [core/src/subscriptions/sync_engine.rs](./core/src/subscriptions/sync_engine.rs), not from the normal manual import path.
4. So the app can live-insert subscription imports, but it still cannot show a true progress indicator for ordinary button/drag imports.

## Problem
File imports provide no progress feedback. After the user selects files, the UI gives no indication that work is happening — no progress bar, no spinner, no file count, nothing. Users don't know if the import is running, stuck, or failed.

## Scope
- Import flow (both button import and drag-and-drop import)
- Progress UI component (progress bar, toast, or status bar indicator)
- Backend: import already processes files sequentially — need to emit per-file progress events

## Implementation
1. Backend: emit a progress event after each file is processed during import, including `{ imported: N, total: M, current_file: "filename" }`.
2. Frontend: show a progress indicator during import. Options:
   - **Progress bar** in the status bar or sidebar
   - **Toast with progress** (e.g. "Importing... 12/50")
   - **Modal with progress bar** for large imports
3. Show completion feedback: "Imported 50 files" toast when done.
4. If any files fail (unsupported format, duplicate, etc.), show a summary: "Imported 47/50 — 3 skipped".

## Acceptance Criteria
1. Starting an import shows immediate visual feedback (spinner, progress bar, or toast).
2. Progress updates during import showing current count vs total.
3. Completion toast showing final count and any skipped/failed files.
4. Works for both button import and drag-and-drop import.

## Test Cases
1. Import 5 files — brief progress indicator, completion toast.
2. Import 50 files — progress bar updates incrementally, completion toast.
3. Import 50 files with 3 unsupported — progress completes, summary shows "47 imported, 3 skipped".
4. Import 1 file — no jarring full-screen progress, just a brief toast.

## Risk
Low. Backend import is already sequential. Adding per-file events is straightforward. Frontend just needs to listen and render.
