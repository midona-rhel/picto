# PBI-143: Color palette extraction and color-based search

## Priority
P2

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Evidence:
1. Eagle has automatic color palette extraction, color-based filtering (DeltaE 2000), find similar color items, re-analyze colors, copy color from palette.
2. Picto extracts dominant color on import but has no color palette display, color filter, or color search.

## Problem
Users cannot search or filter by color, view extracted palettes, or find visually similar images by color.

## Scope
- Backend: store full color palette (5-8 colors per image)
- `src/components/image-grid/ImagePropertiesPanel.tsx` — palette display
- Filter: color filter panel (ties into PBI-128)
- Backend: DeltaE color distance query

## Implementation
1. Extract 5-8 dominant colors on import (already partially done with dominant color).
2. Store as JSON array in `palettes` column: `[{r, g, b, ratio}]`.
3. Inspector: render color swatches, click to copy hex, click to filter by similar.
4. Color filter: select a color, find items with similar palette colors (DeltaE 2000 distance).
5. Re-analyze Colors: context menu to re-run extraction.
6. Sort by HSL within palette display.

## Acceptance Criteria
1. Inspector shows color palette swatches for each image.
2. Click swatch → copies hex to clipboard.
3. Color filter finds images with matching palette colors.
4. Re-analyze updates palette from current file.

## Test Cases
1. Image with red/blue — palette shows red and blue swatches.
2. Click red swatch → filter → shows other images with red.
3. Re-analyze on edited image — palette updates.

## Risk
Medium. Color distance algorithm needed. Palette extraction may need tuning.
