# PBI-115: Flip and rotate tools

## Priority
P2

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Evidence:
1. Preview-only rotate/flip controls are implemented in `src/components/image-grid/DetailWindow.tsx` (toolbar buttons + CSS transform pipeline).
2. Missing from this PBI: dedicated Shift+F/Shift+R bindings, write-to-file mode, and persisted thumbnail/content updates after transform.

## Problem
Users cannot flip or rotate images. Common need for correcting orientation.

## Scope
- `src/components/image-grid/DetailWindow.tsx` — toolbar buttons + CSS transforms for preview mode
- Backend: actual image transform (write rotated/flipped result)
- `src/lib/shortcuts.ts` — Shift+F, Shift+R shortcuts
- Settings: rotation mode preference (preview-only vs write-to-file)

## Implementation
1. Shift+F flips image horizontally (CSS transform for preview, optional write).
2. Shift+R rotates image 90 degrees clockwise.
3. Setting: "Image rotation mode" — preview only (CSS) vs write to file (backend).
4. Toolbar buttons in detail view for flip/rotate.
5. Backend: decode image, apply transform, re-encode, update blob store and thumbnail.

## Acceptance Criteria
1. Shift+F flips image horizontally.
2. Shift+R rotates image 90 degrees clockwise.
3. In write mode, changes persist to file. In preview mode, only visual.
4. Thumbnail updates after write-mode transform.

## Test Cases
1. Shift+F — image flips horizontally.
2. Shift+R twice — image is upside down.
3. Write mode: close and reopen — transform persisted.
4. Preview mode: close and reopen — image back to original.

## Risk
Low-Medium. CSS transforms trivial; backend image writing moderate effort.
