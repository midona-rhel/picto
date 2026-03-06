# PBI-249: Inspector scrollbar shifts content layout

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. When the inspector panel content overflows and a scrollbar appears, the main content shifts to the left to make room for the scrollbar.
2. This causes a visible layout jump when toggling between content that does and doesn't overflow.
3. The scrollbar should overlay the content or the gutter should be reserved so content position stays stable.

## Problem
The inspector panel's scrollbar is not using overlay or reserved-gutter scrollbar behavior. When content grows long enough to trigger a scrollbar, the scrollbar pushes the content leftward, causing a layout shift. This is visually jarring and makes the panel feel unstable.

## Scope
- `src/components/image-grid/ImagePropertiesPanel.tsx` (or equivalent inspector component) — scrollbar CSS

## Implementation
1. Apply `scrollbar-gutter: stable` to the inspector's scrollable container. This reserves space for the scrollbar at all times, so content position never shifts.
2. Alternatively, use `overflow: overlay` (WebKit/Electron) to make the scrollbar float on top of content without affecting layout.
3. If using `scrollbar-gutter: stable`, ensure the reserved space doesn't look awkward when there's no scrollbar — a thin gutter is acceptable.

## Acceptance Criteria
1. Inspector content does not shift horizontally when the scrollbar appears or disappears.
2. Scrollbar is functional and visible when content overflows.
3. Content padding/alignment is stable regardless of overflow state.

## Test Cases
1. Open inspector with short content (no overflow) — no scrollbar, content aligned normally.
2. Open inspector with long content (overflow) — scrollbar appears, content stays in the same position.
3. Toggle between items with short and long metadata — no layout jump.

## Risk
Low. Single CSS property change. `scrollbar-gutter: stable` is supported in Chromium (which Electron uses).
