# PBI-541: Complete Undo/Redo Coverage

## Problem
Multiple user-initiated mutations still lack undo/redo support. These were found via audit after the initial undo/redo pass.

## Remaining Missing Undo/Redo

### Folder Creation (4 locations)
- `useGridHotkeys.ts:132` — Mod+Shift+N creates folder, no undo
- `useGridHotkeys.ts:141` — Alt+N creates subfolder, no undo
- `imageActions.tsx:676` — Context menu "New Subfolder", no undo
- `imageActions.tsx:695` — Context menu "New Folder from Selection", no undo (both create + addFiles)

### SubfolderGrid (5 locations)
- `SubfolderGrid.tsx:194` — Create subfolder, no undo
- `SubfolderGrid.tsx:217` — Rename folder, no undo
- `SubfolderGrid.tsx:233` — Apply folder color, no undo
- `SubfolderGrid.tsx:243` — Apply folder icon, no undo
- `SubfolderGrid.tsx:266` — Folder auto-tags, no undo

### Folder Sort/Reverse (4 locations)
- `useGridFeatureState.ts:114` — Sort folder items, no undo
- `useGridFeatureState.ts:121,130` — Reverse folder items, no undo
- `imageActions.tsx:742-772` — Context menu sort/reverse submenu, no undo

### Other
- `SmartFolderList.tsx:471` — Batch delete smart folders, no undo

## Excluded (intentionally no undo)
- Watch config (set/clear) — configuration, not content mutation
- Thumbnail regeneration — read-side effect, not data mutation
- Import operations — automated
- Permanent delete — irreversible by design

## Implementation Notes
- Follow existing `registerUndoAction` pattern
- Capture previous state before mutation, restore on undo
- For folder creation: undo = delete folder, redo = re-create
- For sort/reverse: undo = reverse sort (or capture original order), redo = re-sort
- On undo failure (e.g. file deleted): silently skip, show "Nothing to undo"
