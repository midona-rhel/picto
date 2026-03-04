# PBI-096: Move waterfall layout and bucket-index build off the main thread

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Layout/bucket data is computed in renderer React path:
   - `/Users/midona/Code/imaginator/src/components/image-grid/CanvasGrid.tsx` (layout + bucket memo usage)
   - `/Users/midona/Code/imaginator/src/components/image-grid/VirtualGrid.tsx` (`computeLayout` and related helpers)
2. User-facing reports mention scrollbar-length jitter and waterfall recalculation instability during transitions/scroll.

## Problem
Large waterfall layout recomputation and index rebuilding on the main thread increases frame drops and contributes to visible scrollbar/geometry jitter.

## Scope
- `/Users/midona/Code/imaginator/src/components/image-grid/VirtualGrid.tsx` (layout helpers extraction)
- `/Users/midona/Code/imaginator/src/components/image-grid/CanvasGrid.tsx`
- new worker path under `/Users/midona/Code/imaginator/src/components/image-grid/`

## Implementation
1. Extract pure layout + bucket-index build into a worker-friendly module.
2. Run layout computation in a dedicated worker for large datasets / waterfall mode.
3. Add incremental append-mode updates:
   - append new positions without full rebuild when possible
4. Keep deterministic fallbacks for small grids and worker-unavailable environments.

## Acceptance Criteria
1. Waterfall layout updates do not block main thread for large page changes.
2. Scrollbar height and tile positions remain stable during scope switch and append.
3. Worker path is transparent to existing UI behavior.

## Test Cases
1. 10k+ images waterfall initial load and append.
2. Scope switch between two waterfall views with different zoom settings.
3. Resize window while scrolling waterfall grid.

## Risk
Medium. Requires careful synchronization of worker results and render state.

