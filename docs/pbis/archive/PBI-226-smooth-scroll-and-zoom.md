# PBI-226: Smooth scroll in media grid

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Eagle supports smooth inertia-based scrolling in the media grid with momentum on trackpad swipe.
2. Picto grid scrolling is functional but lacks smooth inertia and momentum.

## Problem
Scrolling through the media grid feels abrupt. Users expect buttery-smooth inertia scrolling with momentum, especially on trackpads.

## Scope
- `src/components/image-grid/` — smooth scroll with momentum for the virtualized grid
- Must cooperate with the active virtualizer (@tanstack/react-virtual or @egjs/react-infinitegrid)

## Implementation
1. Apply CSS `scroll-behavior: smooth` or a requestAnimationFrame-based scroll interpolation to the virtualized grid container.
2. Ensure smooth scroll cooperates with the virtualizer's item mount/unmount lifecycle — items appearing during momentum scroll must not cause visible layout shifts.
3. Preserve scroll position stability when items load or grid dimensions change mid-scroll.

## Acceptance Criteria
1. Grid scrolling has visible momentum/inertia on trackpad swipe.
2. Scroll-wheel scrolling feels smooth with no snapping or jitter.
3. Performance: smooth scroll maintains 60fps with a 1000+ item grid.
4. No layout shifts or flicker as virtualized items mount during momentum scroll.

## Test Cases
1. Trackpad swipe in grid — scroll continues with momentum after finger lifts.
2. Scroll-wheel in grid — scrolling feels smooth and continuous.
3. Rapid scroll through 1000+ items — no visible frame drops or jank.
4. Scroll while images are still loading thumbnails — no layout jumps.

## Risk
Medium. Smooth scroll must cooperate with the virtualized grid (items mount/unmount during scroll). The chosen virtualizer may need configuration or patching to support interpolated scroll offsets without layout thrashing.
