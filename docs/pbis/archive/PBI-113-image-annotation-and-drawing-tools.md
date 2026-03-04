# PBI-113: Image annotation and drawing tools

## Priority
P3

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Comment Mode with freehand pencil drawing, color picker, eraser, and rectangle annotation comments on images.
2. Picto has no annotation or drawing capability.

## Problem
Users cannot annotate images with drawings, rectangles, or text comments for review/feedback workflows.

## Scope
- New component `src/components/viewer/AnnotationLayer.tsx` — canvas overlay for drawing
- Backend: `core/src/sqlite/annotations.rs` — annotation storage (strokes, rectangles, text)
- `core/src/sqlite/schema.rs` — annotations table
- `src/components/image-grid/DetailWindow.tsx` — toggle between preview and comment mode

## Implementation
1. Annotations table: `annotation_id, file_id, type (stroke|rect|text), data (JSON), color, created_at`.
2. Canvas overlay in detail view for drawing (freehand pencil, rectangles).
3. Color picker for annotation color.
4. Eraser tool.
5. Toggle between Preview Mode and Comment Mode.
6. Rectangle comments: click to place, add text, edit/delete.
7. Render annotation indicators on grid thumbnails (optional toggle via Cmd+Alt+6).

## Acceptance Criteria
1. Can draw freehand on images in comment mode.
2. Can place rectangle annotations with text.
3. Annotations persist across sessions.
4. Annotations visible as overlay on grid thumbnails (toggleable).

## Test Cases
1. Enter comment mode — drawing tools appear.
2. Draw on image — strokes render and persist after closing.
3. Add rectangle comment — text bubble appears at position.
4. Toggle Cmd+Alt+6 — annotation indicators show/hide on grid.

## Risk
Medium-High. Canvas drawing, persistence, and overlay rendering are significant scope.
