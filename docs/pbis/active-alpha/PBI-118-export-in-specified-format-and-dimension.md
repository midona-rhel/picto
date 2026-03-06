# PBI-118: Export in specified format and dimension

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Shift+E for batch convert and export to target format with custom dimensions.
2. Eagle also has Cmd+E for basic export to computer.
3. Picto has no export functionality.

## Problem
Users cannot export files from the library, convert formats, or resize for external use.

## Scope
- New dialog `src/components/dialogs/ExportDialog.tsx`
- Backend: image conversion and resizing
- `src/lib/shortcuts.ts` — Cmd+E (basic export), Shift+E (format/dimension export)

## Implementation
1. Basic export (Cmd+E): copy original files to user-selected folder.
2. Format export (Shift+E): dialog with format selector (PNG, JPG, WebP, AVIF), quality slider, dimension inputs (width/height with aspect ratio lock), output folder picker.
3. Backend: decode → resize → re-encode in target format → write to destination.
4. Progress indicator for batch exports.
5. WebP → PNG/JPG conversion as a special case.

## Acceptance Criteria
1. Cmd+E exports original files to chosen folder.
2. Shift+E opens format/dimension dialog.
3. Batch export works on multiple selections with progress indicator.
4. Format conversion produces valid output files.

## Test Cases
1. Cmd+E with 5 files selected — all 5 copied to chosen folder.
2. Shift+E: convert PNG to JPG at 50% quality — valid JPG produced.
3. Resize 4000x3000 to max 1920 wide — output is 1920x1440.

## Risk
Medium. Format conversion and resizing require image processing pipeline.
