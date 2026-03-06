# PBI-126: Show subfolder content toggle

## Priority
P2

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Evidence:
1. UI toggle and setting for `Show subfolders` exists in `src/components/image-grid/DisplayOptionsPanel.tsx`, and `ImageGrid` wires a `SubfolderGrid` when enabled.
2. Missing from this PBI: recursive descendant-content aggregation in the main grid query (current behavior shows subfolder tiles, not merged descendant file content in parent scope).

## Problem
When viewing a parent folder, users cannot see files from child folders. Must navigate into each subfolder separately.

## Scope
- Backend: recursive folder query (OR all child folder bitmaps)
- `src/lib/shortcuts.ts` — Cmd+Alt+7 shortcut
- Grid controls: toggle button for subfolder content

## Implementation
1. Backend: when subfolder content enabled, query includes all descendant folder bitmaps (walk folder tree, OR bitmaps).
2. Toggle state per-folder or global preference.
3. Cmd+Alt+7 shortcut to toggle.
4. Visual indicator in toolbar when subfolder content is shown.
5. Sidebar folder count reflects subfolder content when enabled.

## Acceptance Criteria
1. Cmd+Alt+7 toggles subfolder content in current folder view.
2. Parent folder shows files from all descendants when enabled.
3. File counts update to include subfolder files.

## Test Cases
1. Parent with 5 files, child with 10 — toggle on shows 15.
2. Toggle off — back to 5.
3. Deeply nested subfolders all aggregate correctly.

## Risk
Low-Medium. Bitmap OR operations already efficient; need folder tree traversal.
