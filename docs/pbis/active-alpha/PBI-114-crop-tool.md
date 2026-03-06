# PBI-114: Crop tool

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Shift+C crop tool with dimension inputs, aspect ratio presets, and "Save as" option.
2. Picto has no crop capability.

## Problem
Users cannot crop images within the application.

## Scope
- New component `src/components/viewer/CropTool.tsx` — crop overlay with handles
- Backend: image cropping (write cropped result to blob store)
- `src/lib/shortcuts.ts` — Shift+C shortcut

## Implementation
1. Crop overlay with draggable corners and edges.
2. Dimension input fields (width x height).
3. Aspect ratio lock (free, 1:1, 4:3, 16:9, original).
4. Apply: overwrite original or save as new file.
5. Keyboard shortcut: Shift+C to enter crop mode.

## Acceptance Criteria
1. Shift+C activates crop overlay on current image.
2. Dragging handles adjusts crop region.
3. Apply writes cropped image and updates thumbnail.
4. Aspect ratio presets constrain proportions.

## Test Cases
1. Shift+C — crop overlay appears on image.
2. Drag corner — crop region resizes proportionally.
3. Apply crop — image updates, dimensions change in inspector.
4. Cancel — no changes made.

## Risk
Medium. Image writing requires backend support (decode, crop, re-encode, update blob store).
