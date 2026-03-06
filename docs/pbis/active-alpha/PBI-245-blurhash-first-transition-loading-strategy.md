# PBI-245: Blurhash-first transition and image loading strategy

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Blurhash data is already stored per-image in the database and available on grid items.
2. Current transitions between views/scopes show either stale cached images from the previous view or blank placeholders during loading.
3. There is no defined loading strategy that uses blurhash as a progressive placeholder during transitions.

## Problem
When transitioning between views (e.g. changing folder, navigating from inbox to all images, entering/exiting detail), the image loading experience is inconsistent. Cached thumbnails from the previous view may linger, blank tiles may flash, or images may pop in at random times. There is no deliberate visual loading sequence that feels intentional and polished.

## Desired behavior

On every view transition (scope change, detail open/close, etc.):

1. **Evict visible image renders** — treat the transition as a clean slate. The image cache itself can persist in memory, but the rendered tiles reset to their blurhash placeholder.
2. **All tiles show blurhash immediately** — the grid populates with blurhash placeholders for every visible item. This is instant (blurhash decode is ~1ms per image) and gives the user immediate spatial feedback: they can see how many images there are, their aspect ratios, and rough color.
3. **After a brief delay (~100ms), images fade in** — each tile loads its actual image (thumbnail or full-size depending on the view context) and crossfades from the blurhash to the real image. The delay is intentional — it prevents a jarring flash when images load instantly from cache, and creates a consistent "settle" feel.
4. **Load priority follows viewport** — images in the visible viewport load first, then images just outside the viewport (pre-fetch buffer). Images far off-screen are deferred.

## Visual sequence

```
t=0ms    View transition triggers
         → All visible tiles snap to blurhash (evict rendered images)
         → Grid layout is complete (positions, sizes known from metadata)

t=100ms  Image loading begins
         → Visible tiles request their image (thumbnail or full-size)
         → As each image loads, it crossfades from blurhash → real image
         → Fade duration ~150-200ms per tile

t=200ms+ Images continue fading in across the viewport
         → Near-viewport tiles load next
         → Off-screen tiles deferred
```

## Scope
- `src/components/image-grid/` — tile rendering, image loading lifecycle
- Image cache layer — add ability to evict rendered state without dropping cached bytes
- Blurhash decode — ensure it's fast enough for batch decode of visible tiles

## Implementation
1. **Tile render states**: Each tile has three visual states:
   - `blurhash` — showing decoded blurhash (default during transition)
   - `loading` — image requested, still showing blurhash
   - `loaded` — real image available, crossfade to it
2. **Transition trigger**: When the view transition controller (PBI-244) fires a transition, broadcast a "reset tiles" signal that sets all visible tiles back to `blurhash` state.
3. **Delayed load**: After the transition settles (layout committed, ~100ms delay), begin loading images for visible tiles. Use `requestIdleCallback` or a microtask queue to avoid blocking the transition animation.
4. **Crossfade**: Each tile renders both the blurhash canvas and the `<img>` element. The `<img>` starts at `opacity: 0` and transitions to `opacity: 1` when loaded. The blurhash fades out simultaneously.
5. **Cache separation**: The in-memory image cache (decoded thumbnails, blob URLs) persists across transitions. The "eviction" only affects the tile render state — when a tile resets to `blurhash`, it doesn't delete the cached image data. This means images that are already cached will fade in almost instantly after the 100ms delay (cache hit), while uncached images will take longer (network/disk fetch).
6. **View-context-aware quality**: Tiles request the appropriate quality based on the current view:
   - Grid view → thumbnail
   - Detail view → full-size
   - QuickLook → full-size

## Relationship to other PBIs
- **PBI-244** (controller-driven view transitions): the transition controller triggers the blurhash reset. This PBI defines what happens visually during the transition.
- **PBI-095** (unify image decode QoS): this PBI defines the loading priority and quality selection. PBI-245 defines the visual strategy.
- **PBI-098** (shared viewer core): the detail viewer uses the same blurhash → full-size loading sequence.

## Acceptance Criteria
1. Every view transition shows blurhash placeholders before real images.
2. Images crossfade from blurhash to real image, not pop in.
3. The 100ms delay is consistent — no images load before the transition settles.
4. Already-cached images still go through the blurhash → fade-in sequence (just faster).
5. Viewport-priority loading: visible images load before off-screen images.
6. The experience feels intentional and polished, not like a loading bug.

## Test Cases
1. Switch from folder A to folder B — all tiles show blurhash, then fade to thumbnails.
2. Switch to a folder with cached images — blurhash shows briefly, cached images fade in fast.
3. Switch to a folder with uncached images — blurhash persists until thumbnails load from disk.
4. Open detail view — blurhash shows at full size, then full-resolution image fades in.
5. Rapid scope changes (click 3 folders quickly) — only the final folder's images load, intermediate transitions are cancelled.
6. Scroll during loading — newly visible tiles start at blurhash, load in priority order.

## Risk
Medium. Blurhash decode for ~50 visible tiles must be fast enough to not delay the transition. Crossfade CSS must not cause layout thrashing. The 100ms delay must be tuned — too short and images pop in before the transition feels settled, too long and the app feels sluggish.
