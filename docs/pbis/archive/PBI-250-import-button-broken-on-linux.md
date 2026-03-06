# PBI-250: Import button not working on Linux

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user on Linux reported: "The Import picture button doesn't seem to work."
2. The button appears clickable but does not trigger a file picker dialog or any visible action.
3. May be an Electron native dialog issue on Linux (file picker not opening), or the button handler not firing.

## Problem
The import button in the UI does not function on Linux. Users cannot import local files through the primary UI affordance. This is a platform parity issue — import works on macOS and Windows.

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

## Test Cases
1. Linux (GNOME): click Import → file picker opens → select files → files imported.
2. Linux (KDE): same flow.
3. Linux (minimal WM, no portal): button click → error toast if dialog unavailable.

## Risk
Medium. Linux dialog support depends on `xdg-desktop-portal` or equivalent. May need to document a system dependency.
