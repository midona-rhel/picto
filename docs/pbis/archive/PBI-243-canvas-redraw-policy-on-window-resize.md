# PBI-243: Canvas redraw policy on window resize

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Resizing the window horizontally triggers a full grid relayout — images are rearranged into new column counts, which is expensive (waterfall/masonry recalculation).
2. There is no debouncing or throttling policy distinguishing vertical from horizontal resize.
3. The grid redraws continuously during drag-resize, causing visible jank.

## Problem
When the user resizes the application window, the canvas/grid redraws on every resize event. Horizontal resizing is particularly expensive because changing the window width changes the number of columns, requiring a full waterfall relayout. This causes jank during resize and wastes CPU on intermediate layouts that are immediately discarded.

## Desired behavior

**Vertical resize (height change only):**
- The grid should extend or shrink to fill the new vertical space immediately.
- New images below the previous viewport boundary should load and appear as space becomes available.
- This is cheap — it's just revealing more of the already-laid-out content or trimming the visible area.

**Horizontal resize (width change):**
- The grid should NOT relayout during the resize drag.
- The relayout should only happen once the mouse stops moving (debounced — e.g. 150-300ms after the last resize event).
- During the resize, the existing layout is preserved, and the grid simply clips or pads at the edges.

**Sidebar resize:**
- Same rules as horizontal window resize — the grid width changes, but relayout is deferred until the drag stops.

## Scope
- `src/components/image-grid/` — resize observer and layout trigger
- Potentially the virtualizer configuration (@tanstack/react-virtual or @egjs/react-infinitegrid)

## Implementation
1. Separate the resize observer into two dimensions: track width and height changes independently.
2. **Height change**: immediately update the virtualized list's visible range (more/fewer items visible). No relayout needed — the column count and item positions don't change.
3. **Width change**: debounce the relayout. During the resize drag, keep the current column count and item positions. Once the mouse is idle for ~200ms, trigger the relayout with the final width.
4. Apply the same debounced-width policy to sidebar resize events.
5. Optionally: show a subtle visual indicator during deferred relayout (e.g. slight opacity change on the grid) to signal that a relayout is pending.

## Acceptance Criteria
1. Dragging the window taller immediately shows more images below — no delay, no relayout jank.
2. Dragging the window wider does NOT trigger continuous relayout — images hold their positions during the drag.
3. After releasing the horizontal resize (mouse stops), the grid relayouts once with the final width.
4. Sidebar resize follows the same debounced policy.
5. No visible jank during any resize operation.

## Test Cases
1. Drag window taller by 200px — new images appear smoothly as space opens.
2. Drag window shorter by 200px — bottom images disappear, no relayout.
3. Drag window wider — no column change during drag; relayout happens ~200ms after release.
4. Drag window narrower — same deferred relayout behavior.
5. Drag sidebar wider/narrower — grid relayout deferred until drag stops.
6. Rapid resize back and forth — only one relayout at the end, not N intermediate ones.

## Risk
Medium. Requires separating width/height in the resize observer and ensuring the virtualizer can handle a deferred width change without visual artifacts. The "reveal more on vertical extend" behavior depends on the virtualizer pre-rendering items below the fold.
