# Code Review Request: Canvas Grid Renderer — Performance & Architecture Pass

## Summary

This changeset covers the canvas grid renderer performance optimization, architectural cleanup, and toolbar rebuild. 14 files changed, 1 new file, ~668 lines added / ~353 removed. All changes compile clean (`npx tsc --noEmit` passes).

## What Changed and Why

### 1. Canvas Rendering Performance (Critical Path)

**Problem:** Scrolling through ~8 images caused visible stutter. Frame profiling showed the draw function itself was fast (~2ms), but the RAF loop was dropping to 40-70fps on 120Hz ProMotion displays.

**Root causes identified and fixed:**

#### a. `getComputedStyle` called every frame (`CanvasGrid.tsx`)
- Was called in the draw function to read background color
- Forces synchronous style recalculation — one of the most expensive DOM calls
- **Fix:** Read once on mount and on resize (ResizeObserver callback), cached in ref

#### b. Thumbnail load completion storms (`thumbnailPipeline.ts`)
- `MAX_CONCURRENT_LOADS` was 12. When many `<img>` elements finished near-simultaneously, each called `createImageBitmap` + `onLoaded` + `drain()`, creating a burst of decode work + multiple redraws in one macrotask
- **Fix:** Reduced to 6 concurrent loads. Added `scheduleNotify()` using `queueMicrotask` to coalesce multiple completions into a single redraw notification

#### c. Per-tile `ctx.clip()` in draw loop (`drawBase.ts`)
- Every tile had `save() → roundRect → clip() → fill + drawImage + stroke → restore()`. Clip paths create GPU stencil masks — expensive per tile
- **Fix:** Removed clip for placeholders (use `roundRect + fill` instead). Kept clip only for the image pass (necessary for rounded corners on photos). Batched all glass borders into a single `beginPath → N × roundRect → stroke` call

#### d. Multi-pass rendering (`drawBase.ts`)
- Was single-pass: placeholder → image → border → badges → stars → text per tile, changing `ctx.font`, `ctx.fillStyle`, `ctx.textBaseline` multiple times per tile
- **Fix:** Restructured into 6 passes grouped by canvas state. Font set once per pass, fillStyle batched. Passes: (1) placeholders, (2) images, (3) batched borders, (4) badges, (5) stars, (6) text

#### e. Allocations every frame (`CanvasGrid.tsx`)
- New `string[]`, `Set<string>`, `Map<string, number>` created every draw frame
- **Fix:** Scratch buffers stored in refs, cleared via `.length = 0` / `.clear()` and reused

#### f. Pipeline request/evict every frame (`CanvasGrid.tsx`)
- `pipeline.request()` and `pipeline.evictExcept()` called even when visible window hadn't changed
- **Fix:** Compare `plan.start`/`plan.end` to previous values, skip pipeline work when unchanged

#### g. RAF cancel+reschedule halving frame rate (`CanvasGrid.tsx`)
- `scheduleRedraw` cancelled pending RAF and rescheduled — if a scroll event arrived mid-frame, it pushed the draw to the next vsync, effectively halving the frame rate
- **Fix:** Changed to guard pattern (`if (rafRef.current != null) return`). Also fixed StrictMode bug where cleanup cancelled RAF but left stale ID in ref, permanently blocking all future draws

#### h. ProMotion variable refresh rate (`CanvasGrid.tsx`)
- macOS ProMotion drops to lower Hz when it doesn't see continuous frame submissions
- **Fix:** During active scroll, pump RAF continuously (not just on dirty). Stop 150ms after last scroll event

#### i. Inline callback props causing effect churn (`CanvasGrid.tsx`)
- `onScrollTopChange`, `onLoadMore`, `onTileClick`, `onFirstPaint` were inline arrow functions from GridScreen — new references every render, causing scroll listener effect to re-run
- **Fix:** Stored all callback props in refs, read via `.current` in handlers. Removed from effect dependency arrays

#### j. `ctx.scale(dpr, dpr)` + `save/restore` per frame (`CanvasGrid.tsx`, `drawBase.ts`)
- Three `save()/restore()` pairs per frame, each copying entire canvas state
- **Fix:** Use `ctx.setTransform(dpr, 0, 0, dpr, 0, -scrollTop*dpr)` directly. No save/restore. drawBase no longer owns the dpr scaling — caller sets the transform

#### k. Integer coordinates (`drawBase.ts`)
- Sub-pixel coordinates trigger expensive anti-aliasing in Skia
- **Fix:** All tile positions truncated via `| 0` before drawing

#### l. Binary search text truncation (`primitives.ts`)
- `truncateText` used linear loop removing one character at a time, calling `measureText` per iteration. 100-char filename = 100 calls
- **Fix:** Binary search — O(log n) `measureText` calls instead of O(n)

#### m. Module-level constants (`primitives.ts`)
- `mimeToExt` map recreated every call. Font constants inline.
- **Fix:** `MIME_TO_EXT` at module scope. `RATING_FONT` extracted. `drawBadge` returns badge width (eliminates redundant `measureText` by caller)

### 2. VRAM-Budgeted Image Cache (`thumbnailPipeline.ts`)

- **Before:** Unbounded `Map<string, ImageBitmap>`. No memory limit.
- **After:** `Map<string, CacheEntry>` with `{ bitmap, bytes, lastUsedAt }`. Tracks total bytes. Enforces 1GB VRAM budget via LRU eviction when over limit.
- Pipeline interface changed from `string[]` to `ImageRequest { hash, displayWidth, displayHeight }` — tiles tell the pipeline their display size. Currently always loads thumbnails, but the abstraction supports quality-tier upgrades without changing the renderer.

### 3. Collection Thumbnail Fix (`canonical.ts`, `api.ts`)

- **Problem:** Collections showed 404 for thumbnails. The pipeline loaded `media://host/thumb/<collection_hash>.jpg` but collections don't have their own file — their thumbnail comes from the primary member.
- **Fix:** Added `thumbnail_hash` field to `CanonicalEntityGridItem`. For singles it equals `entity_hash`. For collections it's the primary member's hash (from the legacy `thumbnail_hash` response field). All image loading/drawing now uses `thumbnail_hash` instead of `entity_hash`.

### 4. Frame Profiler (`frameProfiler.ts` — NEW FILE)

- Zero-allocation hot path: pre-allocated `Float64Array` ring buffer for 120 frames
- 6 instrumented phases: visibilityPlan, hashCollection, pipeline, revealCompute, clear, drawBase
- Per-phase stats: avg/max/p99 over rolling 120-frame window
- FPS counter, dropped frame counter (>8.33ms = missed 120Hz budget)
- `console.warn` on every dropped frame with per-phase breakdown
- Dev overlay in CanvasGrid (bottom-right, `position: fixed`, green monospace, updates every 500ms)

### 5. Scope Transition Fade Fix (`GridScreen.tsx`)

- **Bug:** Grid never faded in after scope transition. `onFirstPaint` fired during `fading_out` phase (before data swap), was ignored because phase wasn't `waiting` yet, and `firstPaintNotifiedRef` was already set so it never fired again.
- **Fix:** Removed `items.length` from the scope-change effect deps (was causing circular re-triggers). Read it from `itemsLengthRef` instead.

### 6. Tile Reveal Lifecycle Fix (`CanvasGrid.tsx`)

- **Bug:** After scope transition completed, tiles would re-fade because `suppressTileReveal` going from `true` to `false` was in the reset effect deps, clearing `lastVisibleSetRef`.
- **Fix:** During suppress mode, `lastVisibleSetRef` is updated with currently visible hashes. Removed `suppressTileReveal` from the reset effect deps.

### 7. Visibility Plan — Preload One Row Above/Below (`visibilityPlan.ts`)

- Extended visible range by one row height above and below the viewport
- Accepts scratch arrays for prefetch indices to avoid per-frame allocations
- Parameters made optional with defaults for backwards compatibility

### 8. Layout Centering with Scrollbar Compensation (`layoutMath.ts`)

- Grid is now centered within `containerWidth + scrollbarWidth` so left visual margin = right visual margin (scrollbar sits inside the right margin)
- `PADDING_X` set to `GAP` (12px) so edge padding matches inter-tile gap

### 9. Scrollbar Styling (`CanvasGrid.module.css`)

- Uses `scrollbar-width: thin` + `scrollbar-color` (standard properties) to preserve native overlay behavior on macOS while matching the app's visual style
- `contain: layout paint` on viewport (not `strict` which collapses to 0 height)
- `will-change: transform` on canvas for GPU layer promotion
- `scrollbar-gutter: stable` to reserve scrollbar space

### 10. Toolbar Rebuild (`GridToolbar.tsx`, `GridToolbar.module.css`)

- Added zoom +/- buttons around the slider (matching legacy `ImageGridControls`)
- Added search input with Cmd+F focus (placeholder only — needs backend filter wiring)
- Removed GPU diagnostics panel (frame profiler overlay is better)
- Layout: `[count] [spacer] [-][slider][+] [view modes] [sort][dir] [search] [loading]`

### 11. Shell Styling (`AppShell.module.css`)

- Sidebar wrapper now has `background: var(--color-surface-1)` so its border matches the titlebar-left border visually
- Titlebar-left and sidebar both use `--color-border-primary` for the right border
- Titlebar-right bottom shadow uses `--color-border-secondary`

## Files Changed

| File | Lines | What |
|------|-------|------|
| `canvas/CanvasGrid.tsx` | 522 | RAF loop, profiler, scroll pump, callback refs, scratch buffers |
| `canvas/frameProfiler.ts` | 227 | **NEW** — per-phase frame profiler with dev overlay |
| `canvas/thumbnailPipeline.ts` | 218 | VRAM budget, ImageRequest interface, batched notifications |
| `canvas/drawBase.ts` | 178 | Multi-pass rendering, clip for images, integer coords |
| `canvas/primitives.ts` | 123 | Binary search truncation, module-level constants, drawBadge returns width |
| `canvas/visibilityPlan.ts` | 70 | Scratch arrays, row preload, optional params |
| `canvas/CanvasGrid.module.css` | 59 | contain, will-change, scrollbar, profiler overlay |
| `layout/layoutMath.ts` | 207 | Scrollbar-compensated centering |
| `GridToolbar.tsx` | 167 | Zoom +/-, search input, layout matching legacy |
| `GridToolbar.module.css` | ~170 | Zoom buttons, search input styling |
| `GridScreen.tsx` | — | Transition fade fix (itemsLengthRef), reveal suppress fix |
| `AppShell.module.css` | — | Sidebar background, border consistency |
| `shared/types/canonical.ts` | — | Added `thumbnail_hash` field |
| `platform/api.ts` | — | Map `thumbnail_hash` from legacy response |

## Known Issues / Cleanup Needed

1. **Debug logging still present** in `CanvasGrid.tsx` — three `console.warn` calls for RAF gap/pump gap/DOM read cost. These are perf investigation logs from this session. Should be removed or gated behind a dev flag before merge.

2. **`visibleHashes` array** in the draw function is still allocated fresh each frame (line ~199 `const visibleHashes: string[] = []`). Should be a scratch ref.

3. **`_paddingX` parameter** in `layoutMath.ts` is unused (replaced by `GAP`-based centering). Should be removed.

4. **Search input** in GridToolbar is UI-only — `searchValue` state is local, not wired to any query. Needs backend filter support.

5. **`container.clientWidth`/`clientHeight`** still read from DOM every draw frame (lines 175-176). The `containerDimsRef` was added but not yet used in the draw function — the switchover was interrupted. This is the remaining source of potential layout thrashing.

6. **Tile placeholder skip** — when `entry && progress >= 1`, the placeholder is skipped. But if the bitmap is later evicted from the VRAM-budgeted cache, the tile will show nothing (no placeholder, no image). The eviction should mark dirty so the next draw sees the missing entry and draws the placeholder.

7. **The `thumbnailPipeline.ts` `RequestGroups` interface** uses `Array<string | ImageRequest>` union type for backwards compatibility with the committed CanvasGrid. Once the CanvasGrid is committed with the new interface, this should be tightened to `ImageRequest[]` only.

## Verification Steps

1. **Compilation:** `npx tsc --noEmit` — passes clean
2. **Scroll performance:** Open DevTools Performance tab, record while scrolling through 20+ images. Frames should be consistently under 8ms
3. **Frame profiler:** Dev overlay shows in bottom-right during dev. Check for dropped frame warnings in console
4. **Scope transition:** Click between folders in sidebar — should fade out old view, fade in new view with no flash of empty canvas
5. **Tile reveal:** Scroll to new area — thumbnails should fade in over 250ms over dominant color placeholder
6. **Collection thumbnails:** Collections should show their primary member's thumbnail, not 404
7. **Layout centering:** Left margin from window edge to first tile should equal right margin from last tile to window edge (accounting for scrollbar)
8. **Zoom +/- buttons:** Click minus/plus in toolbar — tiles should resize in 50px steps
9. **Resize:** Drag window edge — layout should freeze during drag, recompute 150ms after release
