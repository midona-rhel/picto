# PBI-250: Import button not working on Linux

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented / Needs Linux Verification**

Evidence:
1. The Import button is wired in [src/features/grid/ImageGrid.tsx](./src/features/grid/ImageGrid.tsx) and calls the Electron dialog bridge through [src/platform/nativeIntegration.ts](./src/platform/nativeIntegration.ts).
2. The Electron side exposes a generic open dialog handler in [electron/ipc/registerHandlers.mjs](./electron/ipc/registerHandlers.mjs) using `dialog.showOpenDialog(...)`.
3. The frontend now has visible error handling around the import flow (`notifyError(err, 'Import Failed')`) instead of a silent no-op if the dialog or import throws.
4. What is still missing is an actual Linux-specific reproduction or compatibility fix. From code alone, there is no platform-specific branch explaining or fixing the original Linux report.

## Problem
The reported Linux failure has not been reproduced or fixed in a platform-specific way. The current code path looks correct, but this PBI still needs Linux validation before it can be closed.

## Scope
- Import button click handler — verify it triggers Electron's native file dialog
- Electron main process — verify `dialog.showOpenDialog` works on Linux
- Possible fallback: if native dialog fails, show an error instead of silent failure

## Implementation
1. Reproduce on Linux — identify whether the button handler fires, whether the IPC message reaches main, and whether the native dialog opens.
2. If the native dialog fails on certain Linux desktop environments (e.g. missing `zenity` or `kdialog`), document the dependency or provide a fallback.
3. Ensure the button always provides feedback — if the dialog fails to open, show an error toast.

## Acceptance Criteria
1. Import button opens a file picker dialog on Linux.
2. Selected files are imported into the library.
3. If the dialog cannot open, an error message is shown instead of silent failure.
4. Linux verification evidence is recorded so this does not remain a speculative bug.

## Test Cases
1. Linux (GNOME): click Import → file picker opens → select files → files imported.
2. Linux (KDE): same flow.
3. Linux (minimal WM, no portal): button click → error toast if dialog unavailable.

## Risk
Medium. Linux dialog support depends on `xdg-desktop-portal` or equivalent. May need to document a system dependency.
