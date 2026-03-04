# PBI-136: Thumbnail background options

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has per-item thumbnail background: none, white, black, gray, checkerboard grid.
2. Picto renders all thumbnails on default background.

## Problem
Transparent images (PNG, SVG) render poorly without a visible background. Users cannot see boundaries of white-on-white images.

## Scope
- Backend: `thumb_bg TEXT` column on files table
- `src/components/image-grid/CanvasGrid.tsx` — render background before thumbnail
- Context menu: "Thumbnail Background" submenu

## Implementation
1. Add `thumb_bg TEXT DEFAULT NULL` to files table (null = default, 'white', 'black', 'gray', 'grid').
2. Canvas rendering: draw background rect/pattern before drawing thumbnail.
3. Checkerboard grid: alternating light/dark squares pattern (standard transparency grid).
4. Context menu: "Thumbnail Background" → None / White / Black / Gray / Grid.
5. Global default in settings (applies when per-item is null).

## Acceptance Criteria
1. Right-click → Thumbnail Background → Black: dark background behind thumbnail.
2. Grid pattern shows standard checkerboard for transparent images.
3. Setting persists per-item.

## Test Cases
1. Transparent PNG on black background — clear boundaries visible.
2. White image on checkerboard — easy to see edges.
3. Reset to none — default background restored.

## Risk
Low. Canvas draw operations + DB column.
