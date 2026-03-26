# Grid Parity Checklist

Reference: PBI-592 (grid screen rebuild)
Fixtures: `src/test-harness/fixtures/grid.ts`
Last evaluated: 2026-03-25

## Current state

The grid screen is a working canvas-based renderer. It is **not yet activatable**
per PBI-592 because the selection model and parity confirmation against fixtures
are not in place. The data path uses the canonical `query_entity_view` command
through `ApplicationEngine` → `LibraryDatabase`. Both loading and reconcile
use the same backend path.

## Toolbar

- [x] Scope label shows current view name (derived from sidebar nodes)
- [x] Item count displays with tabular-nums formatting
- [x] Sort field selector with 6 options (Date Added, Date Created, Date Modified, Name, Rating, Size)
- [x] Sort direction toggle (asc/desc) with icon indicator
- [x] Loading indicator (pulsing dot) when data is being fetched
- [x] Changing sort reloads the grid via controller
- [x] View mode toggle (waterfall / grid / justified) with segmented button group
- [x] Zoom minus/plus buttons with slider (100–900px, step 50)
- [x] Search input field (Cmd+F focus, wired to query via debounced `search_text` filter)
- [ ] Filter button placeholder — **not implemented**

## Tile rendering (canvas)

- [x] Thumbnail loading via `media://` protocol using `<img>` → `createImageBitmap`
- [x] Dominant color placeholder fill when thumbnail not yet loaded
- [x] Video duration badge (top-right, dark rounded rect, white text)
- [ ] Audio indicator for video tiles — **not implemented** in canvas draw path
- [x] Collection member count badge
- [x] Extension badge (bottom-right, when `showExtension` enabled)
- [x] Rating stars at top-left (gold, repeated ★ character)
- [x] Name text below tile (truncated with ellipsis, when `showName` enabled)
- [ ] Resolution text below name — **not implemented**, no `gridShowResolutionAtom`
- [x] Glass inner border (`rgba(255,255,255,0.15)`, 1px inset, 8px radius)
- [x] Center-crop image drawing (`drawImageCover`) with rounded clip
- [x] Animated thumbnail reveal (fade-in 250ms over dominant color placeholder)
- [x] Staggered reveal timing (max 54 concurrent fades)
- [x] Collection thumbnails use primary member hash (no 404 for collections)
- [x] Placeholder skipped once image fully revealed (no wasted draws)
- [x] VRAM-budgeted image cache (1GB cap, LRU eviction)

## Layout

- [x] Waterfall (masonry) layout — shortest-column placement
- [x] Grid (uniform square) layout
- [x] Justified (row-fill) layout with last-row fix
- [x] Column count derived from container width and target size
- [x] Rounded corners via canvas `roundRect` clip
- [x] Hover ring on overlay canvas (blue, 2px)
- [x] Scrollbar-gutter: stable
- [ ] Resize-freeze during window drag — **not implemented**, recomputes every ResizeObserver tick

## Pagination

- [x] First page loads automatically on scope change
- [x] Scroll-to-load fetches next page using cursor (400px threshold)
- [x] Loading state displayed during fetch
- [x] Version token prevents stale page results from overwriting newer data

## States

- [x] Empty state: icon + "No items" message
- [x] Error state: error message + Retry button
- [x] Non-grid surface: "This view is not available yet"

## Reconcile / settle

- [x] Metadata-only changes patch visible rows in place
- [x] Membership changes use ReplaceWindow (one round-trip)
- [x] Stale reconcile results are version-gated
- [x] Grid settle is scope-aware and ignores unrelated events
- [x] Compiler batch triggers refresh for smart folder / system scopes

## Data path (known constraints)

- [x] Canonical `query_entity_view` command — live, both load and reconcile use same backend path
- [x] Smart folder scope passes `{ kind: 'smart_folder', id }` — resolved via bitmap in engine
- [x] Search text filter passed through to canonical query
- [x] Response items are canonical `EntityGridItem` with `thumbnail_hash` — no legacy mapping needed

## Selection — **not implemented** (PBI-593 / PBI-584)

- [ ] Click-to-select single tile
- [ ] Multi-select (Cmd/Ctrl+click, Shift+click, Cmd+A)
- [ ] Selection state drives overlay canvas rendering
- [ ] Selection-driven actions target what user actually selected
- [ ] Virtual selection / select-all semantics

## Interaction

- [x] Click hit-testing (identifies tile under pointer)
- [x] Hover state tracking (overlay canvas redraw)
- [ ] Tile context menu (right-click actions) — **not implemented**
- [ ] Keyboard navigation (arrow keys) — **not implemented**
- [ ] Drag from grid to sidebar folder — **not implemented**

## Not yet started

- [ ] Per-scope view preferences (load/save via `get_view_prefs` / `set_view_prefs`)
- [ ] Scroll-phase-aware prefetch (idle/slow/fast)
- [ ] Full-quality image lifecycle (only thumbnails are loaded currently)
- [ ] Shared preview primitive (tile visuals are feature-owned, not shared per PBI-594)
- [ ] Parity harness renders rebuilt grid against fixtures (currently JSON-only)

## PBI activation blockers

1. ~~Legacy `get_grid_page_slim` bridge~~ — replaced with canonical `query_entity_view`
2. Selection model must exist (PBI-593, gated by PBI-584)
3. Parity harness must render rebuilt grid surfaces, not just fixture JSON (PBI-590)
4. Smart folder scope mapping must be correct
5. Shared preview primitive contract must be demonstrated (PBI-594)
