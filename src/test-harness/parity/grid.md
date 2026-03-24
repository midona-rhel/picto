# Grid Parity Checklist

Reference: PBI-592 (grid screen rebuild)
Fixtures: `src/test-harness/fixtures/grid.ts`
Last evaluated: 2026-03-24

## Toolbar

- [x] Scope label shows current view name (All, Inbox, Trash, Folder, Smart Folder, etc.)
- [x] Item count displays with tabular-nums formatting
- [x] Sort field selector with 6 options (Date Added, Date Created, Date Modified, Name, Rating, Size)
- [x] Sort direction toggle (asc/desc) with icon indicator
- [x] Loading indicator (pulsing dot) when data is being fetched
- [x] Changing sort reloads the grid via controller

## Tile rendering

- [x] Image tiles show thumbnail via media:// protocol
- [x] Tiles show dominant_color_hex as background placeholder
- [x] Video tiles show duration badge (top-right)
- [x] Video tiles with has_audio show audio indicator
- [x] Collection tiles show member_count badge
- [x] Tiles without thumbnails show type placeholder text
- [x] Name label renders at bottom with gradient overlay
- [x] Rating stars render at top-left when rating > 0

## Layout

- [x] Grid uses auto-fill columns with min 180px
- [x] Tiles are square (aspect-ratio: 1)
- [x] Tiles have rounded corners
- [x] Hover shows primary-color ring
- [x] Scrollbar-gutter: stable

## Pagination

- [x] First page loads automatically on scope change
- [x] Scroll-to-load fetches next page using cursor
- [x] Loading state shows during fetch

## States

- [x] Empty state: icon + "No items" message
- [x] Error state: error message + Retry button
- [x] Non-grid surface: "This view is not available yet"

## Reconcile

- [x] Metadata-only changes patch visible rows in place
- [x] Membership changes use ReplaceWindow (one round-trip)
- [x] Stale reconcile results are version-gated
- [x] Grid settle is scope-aware and ignores unrelated events

## Not yet implemented (follow-up PBIs)

- [ ] Click-to-select single tile — PBI-593
- [ ] Multi-select (Cmd/Ctrl+click, Shift+click, Cmd+A) — PBI-593/584
- [ ] Tile context menu (right-click actions) — PBI-593
- [ ] Filter bar — follow-up PBI-592 chunk
- [ ] Drag from grid to sidebar folder — follow-up
- [ ] Keyboard navigation (arrow keys) — follow-up
- [ ] Rating shortcut keys (0-5) — follow-up
- [ ] Delete key → trash — PBI-593
- [ ] View mode toggle (grid/list) — follow-up
- [ ] Tile size slider — follow-up

## Accepted differences from legacy

- Sort selector is a native `<select>` instead of a custom dropdown — intentional simplification for this chunk
- No filter bar yet — follow-up chunk
- No tile name overlay toggle — follow-up
- No view mode toggle — follow-up
