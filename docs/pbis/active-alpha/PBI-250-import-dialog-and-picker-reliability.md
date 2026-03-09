# PBI-250: Import dialog and picker reliability

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented / Needs Cross-Platform Verification**

Evidence:
1. The Import button is wired in [src/features/grid/ImageGrid.tsx](./src/features/grid/ImageGrid.tsx) and calls the Electron dialog bridge through [src/platform/nativeIntegration.ts](./src/platform/nativeIntegration.ts).
2. The Electron dialog bridge in [electron/ipc/registerHandlers.mjs](./electron/ipc/registerHandlers.mjs) has been tightened to parent dialogs to the sender window and normalize dialog properties instead of loosely merging them.
3. The frontend now has visible error handling around the import flow (`notifyError(err, 'Import Failed')`) instead of a silent no-op if the dialog or import throws.
4. What is still missing is explicit verification that the import picker behaves correctly across the supported desktop environments and window states.

## Problem
The original report was not actually Linux-specific. The real problem area is import dialog reliability in Electron: making sure the file picker opens consistently from the main window, returns the expected file list, and fails loudly rather than silently when the host dialog cannot be shown.

## Scope
- Import button click handler
- Electron dialog bridge
- Dialog parenting and option normalization
- Error feedback when the picker cannot open

## Implementation
1. Verify the Import button always triggers the Electron file picker from the active window.
2. Keep the dialog parented to the sender window so modality/focus behaves correctly.
3. Normalize picker properties so the open-file flow is deterministic instead of depending on loosely merged options.
4. Ensure the button always provides feedback — if the dialog fails to open, show an error toast.

## Acceptance Criteria
1. Import button opens a file picker dialog from the active app window.
2. Selected files are imported into the library.
3. If the dialog cannot open, an error message is shown instead of silent failure.
4. Verification evidence is recorded for at least one non-macOS environment so this does not remain speculative.

## Test Cases
1. Main window: click Import → file picker opens → select files → files imported.
2. Window focused after other app interaction: click Import → picker still appears parented to the current app window.
3. Dialog failure path: import click → error toast instead of silent no-op.

## Risk
Medium. Desktop dialog behavior can still vary by environment, but this is an app-level reliability issue rather than a Linux-only bug.
