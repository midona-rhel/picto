# PBI-600 — Tooltip & Keyboard Shortcut Audit

Add `KbdTooltip` to all interactive buttons and display keyboard shortcuts in all context menu items.

## Buttons Needing KbdTooltip

### AppShell Titlebar
- [x] Back button → `KbdTooltip label="Back" shortcut="Mod+["`
- [x] Forward button → `KbdTooltip label="Forward" shortcut="Mod+]"`

### GridToolbar
- [x] Zoom Out → `KbdTooltip label="Zoom out" shortcut="-"`
- [x] Zoom In → `KbdTooltip label="Zoom in" shortcut="+"`
- [x] View button — removed bare `title=` (tooltip via menu)
- [x] Filter button — removed bare `title=` (tooltip via menu)

### Inspector
- [x] Add tag button → `KbdTooltip label="Add Tags" shortcut="T"`
- [x] Add folder button → `KbdTooltip label="Add to Folder" shortcut="Shift+F"`

### Sidebar
- [ ] Folders section add → `KbdTooltip label="New Folder" shortcut="Mod+Shift+N"`
- [ ] Smart Folders section add → `KbdTooltip label="New Smart Folder"`

### OverlayShell
- [ ] Pin button → `KbdTooltip label="Pin"` / `"Unpin"`

## Context Menu Items — Shortcuts Added

### Grid Tile Menu
- [x] Accept → `inbox.accept` (Enter)
- [x] Reject → `inbox.reject` (Backspace)
- [x] Add Tags → `organize.addTag` (T)
- [x] AI Tagger → `organize.autoTag` (Mod+Shift+A)
- [x] Batch Rename → `edit.batchRename` (Mod+Shift+R)

## New Shortcut Definitions Added (shortcuts.ts)

- [x] `edit.batchRename` → `Mod+Shift+R`
- [x] `organize.autoTag` → `Mod+Shift+A`

## Shortcuts Defined But Not Displayed (future work)

- `nav.allActive` (Mod+1), `nav.inbox` (Mod+2), `nav.untagged` (Mod+3), `nav.trash` (Mod+4)
- `nav.search` (Mod+F)
- `view.layoutGrid` (Alt+1), `view.layoutWaterfall` (Alt+2), `view.layoutJustified` (Alt+3)
- `view.toggleTileName` (Mod+Alt+4)
- `file.newFolder` (Mod+Shift+N)
- `file.import` (Mod+I)
