# PBI-119: Combine/merge images

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Cmd+Shift+M to merge multiple images horizontally/vertically with alignment, margin, and stretch options.
2. Picto has no image combining.

## Problem
Users cannot create collages or comparison images by merging multiple images.

## Scope
- New dialog `src/components/dialogs/CombineImagesDialog.tsx`
- Backend: image compositing (stitch images together)
- `src/lib/shortcuts.ts` — Cmd+Shift+M shortcut

## Implementation
1. Dialog: direction (horizontal/vertical), alignment (top/center/bottom or left/center/right), margin between images, background color, stretch-to-fit toggle.
2. Preview in dialog showing combined result.
3. Backend: decode all selected images, compute combined canvas, draw each image, encode result.
4. Save as new file in library or export to disk.

## Acceptance Criteria
1. Cmd+Shift+M with 2+ images selected opens combine dialog.
2. Horizontal and vertical stitching produce correct layouts.
3. Margin and alignment options work as expected.
4. Combined image saved as new library item.

## Test Cases
1. Select 3 images, Cmd+Shift+M, horizontal — produces wide combined image.
2. Vertical with 10px margin — images stacked with gaps.
3. Different-sized images with center alignment — aligned correctly.

## Risk
Medium. Image compositing logic + preview rendering.
