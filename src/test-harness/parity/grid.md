# Grid Parity Checklist

Reference: PBI-592 (grid screen rebuild)
Fixtures: `src/test-harness/fixtures/grid.ts`

## Tile rendering

- [ ] Image tiles show thumbnail with correct aspect ratio
- [ ] Video tiles show thumbnail with duration badge overlay
- [ ] Video tiles with has_audio=true show audio indicator
- [ ] Animated GIF tiles show frame_count indicator
- [ ] Collection tiles show member_count badge
- [ ] Tiles without thumbnails (has_thumbnail=false) show placeholder
- [ ] Tiles with dominant_color_hex use it as placeholder background
- [ ] Tiles without name show hash-based fallback or no label
- [ ] Rating stars render on tiles when rating is set
- [ ] Status indicators: inbox (status=0) vs active (status=1)

## Layout

- [ ] Masonry/grid layout adapts to container width
- [ ] Target tile size respects view preferences
- [ ] View mode toggle (grid/list) works
- [ ] Tile size slider works
- [ ] Sort indicator shows current sort field and direction
- [ ] Filter bar shows active filters

## Pagination

- [ ] First page loads automatically on scope change
- [ ] Scroll-to-load fetches next page using cursor (not offset)
- [ ] next_cursor=null means no more pages (no further fetches)
- [ ] total_count displays in header/toolbar
- [ ] Loading state shows skeleton or spinner during fetch
- [ ] Error state renders retry option

## Selection

- [ ] Click selects single tile (deselects others)
- [ ] Cmd/Ctrl+click toggles individual selection
- [ ] Shift+click selects range
- [ ] Cmd/Ctrl+A selects all (virtual select-all, not hash enumeration)
- [ ] Selection count in toolbar matches actual selected count
- [ ] Virtual select-all count matches total_count from backend

## Query model

- [ ] Scope changes (sidebar navigation) rebuild query with correct base_scope
- [ ] Sort changes update query sort field/direction
- [ ] Filter changes update query filters
- [ ] Search text updates base_scope to kind=search
- [ ] Rating filter uses FilterOp (eq/gte/lte/gt/lt)
- [ ] Entity type filter supports multi-select (image, video, audio, collection)
- [ ] Tag filter supports include and exclude modes
- [ ] Date range filters work for date_created, date_added, date_modified

## Edge cases

- [ ] Empty results: show "no items" message, not blank screen
- [ ] Single item: renders correctly, no pagination artifacts
- [ ] Scope with 10k+ items: no lag on first page, smooth scroll pagination
- [ ] Grid refreshes correctly after state_changed event with entity_hashes
- [ ] Grid handles optimistic removals (pending_removal_hashes) without flicker
- [ ] Grid handles pending insertions (undo restore) correctly

## Legacy behavior to preserve

- [ ] Tile hover shows subtle border/shadow
- [ ] Context menu on right-click with correct actions per entity type
- [ ] Drag from grid to sidebar folder triggers folder membership add
- [ ] Keyboard navigation (arrow keys) works within the grid
- [ ] Rating shortcut keys (0-5) apply to selected items
- [ ] Delete key moves selected items to trash
