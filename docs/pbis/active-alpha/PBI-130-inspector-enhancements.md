# PBI-130: Inspector enhancements (folder membership, color palette, EXIF)

## Priority
P2

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Evidence:
1. Eagle inspector shows: folder membership list, extracted color palette (with copy/find similar), EXIF camera info, source URL, multi-selection batch inspector.
2. Picto inspector shows basic metadata but lacks folder membership, color palette, and EXIF.

## Problem
Inspector panel is missing key metadata: which folders contain a file, extracted color palette, and camera/EXIF data.

## Scope
- `src/components/image-grid/ImagePropertiesPanel.tsx` — new sections
- Backend: color palette extraction (already partially done for sort), EXIF reading
- `src/hooks/useInspectorData.ts` — additional data queries

## Implementation
1. Folder membership section: list all folders containing the file, with click to navigate.
2. Color palette section: show extracted colors as swatches, click to copy hex, click to filter by similar color.
3. EXIF section: camera model, aperture, shutter speed, ISO, focal length (read from image metadata).
4. Source URL: clickable link to open in browser (if file has associated URL).
5. Multi-selection inspector: show count, batch-editable fields (tags, rating, folders).

## Acceptance Criteria
1. Inspector shows folder membership with navigation links.
2. Color palette swatches displayed with copy-on-click.
3. EXIF data shown for photos with camera metadata.
4. Multi-selection shows batch view.

## Test Cases
1. File in 2 folders — both shown in inspector, clickable.
2. Image with rich colors — palette shows 5-8 dominant colors.
3. Photo with EXIF — camera, aperture, ISO displayed.
4. Select 10 files — inspector shows "10 items selected" with batch fields.

## Risk
Medium. EXIF reading needs library (exif crate). Color palette may already exist.
