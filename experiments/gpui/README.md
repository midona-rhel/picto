# Picto GPUI frontend experiment (#163)

A native GPUI frontend for Picto, built to reach the same behavior and look as
the Electron app. It is not a replacement yet.
Issue: https://github.com/midona-rhel/picto/issues/163

## Run safely

```sh
experiments/gpui/run.sh /path/to/Main.library
```

The launcher uses SQLite's online backup to create a disposable database under
`/tmp/picto-gpui-library.*`. It reads existing thumbnails and originals in place
without copying or changing blobs. The app refuses databases without the
snapshot marker. Edits and undo/redo affect only that copy. The original library
and the main checkout are not modified. Keep the snapshot until the app is closed.

The generated macOS bundle is `experiments/gpui/target/Picto GPUI.app`.

## Toolchain and GPUI version

The experiment has its own Cargo workspace and lockfile. It depends on
`gpui-pre` and `gpui-pre-platform` `=0.3.12`, the 9 November 2026 crates.io
snapshot of upstream GPUI (`zed-industries/zed`). The official `gpui` crate has
not been published since 0.2.2 (October 2025). GPUI promises no API stability
between snapshots, so the version is pinned exactly and upgraded deliberately.

The snapshot needs Rust 1.98, the version `rust-toolchain.toml` pins. Without
rustup, install that toolchain anywhere and put its `bin` first on `PATH`;
`.toolchain/` in this directory is ignored by Git for that purpose.

## Implemented

- Existing `picto_library` query, details, smart-folder, rating, rename, notes
  and undo owners. Database work runs on one worker thread, never in the
  rendering callback.
- Grid geometry ported from `layoutMath.ts` (waterfall, grid, justified). Each
  scope opens with its saved view from the library's `view_pref` rows: sort,
  zoom, spacing, layout mode and which labels show. On the same library and
  window size, tiles land where the Electron grid puts them.
- Tiles drawn to the rules of `drawBase.ts`: media rect with hairline ring,
  format, duration and collection-count badges, selection stroke outside the
  rect, optional name and resolution rows. Only the grid mode letterboxes by
  choice; an item of unknown shape lays out as 3:2 and fills its tile.
- Grid selection as the Electron grid has it: nothing selected when a scope
  opens, a click selects one, Cmd-click toggles, Shift-click and Shift with a
  move select a range, Cmd+A selects all, and Escape or a click on empty
  space clears. Only the image takes the pointer, not the text rows under it.
  The hovered image shows the magnifier corner, except on video and while
  scrolling. "Return to Top" rises once the grid has scrolled 200px.
- Sidebar with the Electron row order (All, Inbox, Recently Viewed,
  Uncategorized, Untagged, Tags, Random, Subscriptions, Duplicates, Trash),
  Folders and Smart Folders sections with counts, hierarchy with tree guide
  lines, disclosure triangles that turn over 200 ms, and section headers that
  fold their section. The filter keeps each matching folder with its
  ancestors and opens every branch. Random shuffles the All scope with one
  seed per session.
- Smart-folder groups (a smart folder with no rules) as the Electron sidebar
  shows them: the grid glyph, no count, and a click that folds the group
  rather than opening it. Folders and smart folders show the icon and colour
  the user chose, from the icon picker's registry (`assets/Icon*.svg`).
- Hover and pressed states from the Electron styles, with their timings: icon
  buttons (hover, pressed, toggled, disabled), sidebar rows and section
  headers (title pill, chevron, add button), the library switcher's highlight,
  chips, the inspector's sections and action rows, the scrollbar thumb, and
  pointer cursors on everything that takes a click.
- The grid's context menu as `ContextMenu.tsx` draws it: glass panel at the
  pointer with a 12px margin from the window's edges, icons, shortcut hints,
  separators, a 30 ms row highlight shared by the pointer and the arrow keys,
  Enter and Escape, and an 80 ms fade when dismissed. A press outside closes
  it and goes no further. Its rows are the ones this prototype can perform:
  open with the default app, reveal, copy path and name, rename, move to
  trash, and restore in the Trash.
- The library switcher opens `LibrarySwitcherPopover` under it: a glass
  panel 8px below the button, kept inside the window, that grows in from
  96% over 70 ms, closes on Escape (also from its search field) and on a
  press outside it and the button, which still reaches what was pressed.
  While it is open the button keeps its highlight on the active surface
  with the glyph lit. It lists the libraries the Electron app knows, read
  (never written) from its `config.json`: pinned first, then the history,
  missing folders left out, each with its cover, name and folder, the open
  one ringed and checked. The search field ranks names as `searchRank.ts`
  does. `ui::popover` is the shared frame and `ui::library_avatar` the cover
  disc both the button and the rows use.
- Tooltips as `KbdTooltip` draws them: below the control after 400 ms, with
  the shortcut's keys as badges.
- Text fields with their focus states: the toolbar search, the sidebar filter
  with its clear button, and the inspector's name and notes. One-pixel
  blinking caret, text that scrolls to keep the caret in view, and an
  ellipsis when a field is not being edited.
- The Electron shell's layout: a 48px titlebar row whose left block keeps the
  sidebar's width, over a body whose rails slide away (sidebar 50 ms,
  inspector 100 ms) while the grid takes the freed width at once. The panel
  button and Tab toggle both rails; Cmd+Alt+1 and Cmd+Alt+2 toggle one.
- The app owns titlebar dragging on macOS. The titlebar strips move the
  window and controls inside them do not, as the Electron shell's drag
  regions work.
- Back and Forward step through places as a browser does. An opened item is
  a place: Back returns to the grid it was opened from, scrolled where it
  was. A destination gone back to shows what it held and where it was
  scrolled; one chosen afresh opens at its top. Choosing the destination
  already showing does nothing.
- No empty frames while data is on its way: the inspector keeps the previous
  item until the next one's details arrive, selecting the selected item asks
  for nothing, and a destination's grid stays hidden behind its fade until
  its first page is there.
- The window's rim (`.shellMac::after`), the library cover cropped to the
  frame stored beside it and set into the rail with its bevel, and Roboto's
  medium and semibold as static files cut from the bundled variable font
  (`assets/Roboto-*.ttf`, made with fontTools' instancer).
- Toolbar with Back and Forward over visited destinations, zoom, Picto's own
  16px glyphs, and native editable search (selection, clipboard, IME). Fewer
  than three trimmed characters leave filtering off. Query generations prevent
  an old result replacing a newer query.
- Real paginated results (500 per page, loaded near the end) and
  viewport-only tiles. Thumbnails load six at a time in the order of
  `thumbnailPlan.ts`: the viewport before the margin around it, nearest its
  middle first. A load for a tile that scrolled away is abandoned. Decoded
  thumbnails beyond the visible band are kept for scrolling back, up to 192,
  and one that a view is showing is never let go.
- Keyboard movement by the rules of `useGridArrowNav.ts`: arrows and WASD,
  Home and End, Page Up and Page Down, and scrolling the target into view.
- Sidebar rows fade their highlight over 50 ms and icon buttons over 150 ms.
  A dismissed context menu fades out over 80 ms.
- Motion with the Electron timings. Thumbnails follow the reveal contract of
  `thumbnailRevealTracker.ts`: 100 ms after entering the 500px margin, a 500 ms
  reveal in which the image fades in over its dominant colour and that colour
  then fades out. Changing destination follows `WorkspaceSurface.tsx`: 170 ms
  fade-out on `--ease-default`, hidden until the first page is ready, 170 ms
  fade-in; clicks during the fade-out retarget it.
- Inspector for the selected item: fitted preview with its format badge,
  colour palette (a click adds that colour to the grid's filters), name,
  notes, source address, folders and tags as chips that can be removed, a
  rating that previews under the pointer, and the properties the Electron
  inspector lists. Its left edge drags between 260 and 550px.
- The media viewer as `MediaView.tsx` has it: the media fills the pane, and
  the titlebar's content switches to the viewer toolbar (back to the grid,
  "n / total", the zoom percentage with its menu of levels and a right-click
  between fit and actual size, the logarithmic zoom slider, Fit to window,
  Actual size, and Previous/Next that disable at the ends), with their
  tooltips and shortcuts. Leaving the viewer for another destination fades
  the titlebar's content with the grid over 170 ms.
- Images zoom and pan by the rules of `useImageZoom.ts`: they open fitted;
  the pointer wheel and trackpad zoom about the pointer (wheel notches glide
  150 ms by 1.25×), pinch zooms, `+`/`=` and `-` glide by 1.25×, `` ` `` or
  Shift+F fits, Cmd+0 shows actual size; a drag pans with the grab and
  grabbing cursors. The navigator appears 80 ms after the image overflows
  the pane; a press or drag in it moves the view there. A step past either
  end shows "First item" or "Last item" for 800 ms. The previous image stays
  until the next thumbnail has decoded; the original replaces the thumbnail
  once decoded, as `ImageCrossfadeFrame` swaps them.
- A collection's members step with the bottom strip of
  `QuickLookNavigation` (48×32 buttons, monospace counter).
- Space, Enter or a double-click opens and closes the viewer, Escape closes
  it. Originals decode off the drawing thread; stale loads are fenced.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).

## Not yet equivalent to Picto

- Tags, Subscriptions and Duplicates are sidebar rows without screens.
- Drawn with their hover states and tooltips but not wired: Settings, View
  options, the section headers' add buttons, the inspector's Add to Folder,
  Add Tags, Export, source-address list and Pin, and in the library
  switcher's popover choosing another library, Create Library, Open Library
  and Library Manager (those rows only close the popover). Each has a `TODO`
  where its action belongs. The Filter button only clears a
  filter the inspector set; there is no filter bar.
- With nothing selected the inspector is empty; the Electron one describes
  the scope. With several items selected it shows the last one, not the
  stacked summary.
- Not in the grid yet: rubber-band selection, dragging items out or to
  reorder, the hover preview of the original, broken-thumbnail, font and
  audio tiles, inline rename, and the scope-specific empty states.
- Not in the sidebar yet: the Subscriptions activity dot, rename in place,
  context menus, multi-selection and drag reorder.
- No video or audio playback, PDF, text, EPUB, CBZ, DjVu, JPEG XL or font
  viewers yet. Word and PowerPoint viewing is out of scope: those files import,
  keep their embedded thumbnail and open in the default application.
- Collection members use previous/next controls, not the continuous collection
  scroll surface. Preview is embedded, not the three production shells.
- Context menus have no submenus or search row yet, exist only for grid
  items, and are drawn inside the window, so they cannot extend past its
  edge. "Delete Permanently" is shown disabled: the prototype reads the
  library's files in place and must never delete them.
- The library popover's glass has no blur, and only its surface scales in;
  its rows appear at full size. A library's folder has no hover title. A
  cover is cropped once per session, so a cover changed elsewhere meanwhile
  shows the old one. A library with an icon but no cover shows the books
  glyph, not its icon.
- No import/export, subscriptions, cloud, localization or site login.
- Viewer: the breadcrumb and "Edit group" belong to the collection editor,
  which does not exist yet, so neither is ever shown; collections step
  through members with the bottom strip instead of the reader surface. No
  transparency checkerboard (off by default in Picto), no per-image zoom
  memory, no viewer context menu, rating or copy shortcuts, and no video
  shortcuts (there is no video playback). Next stops at the last loaded item
  until the next page arrives. The very first opening shows one empty frame
  while the pane is measured. When the viewer closes by going to another
  destination, the pane shows the fading grid rather than the fading viewer.
- Motion still to port: modal enter/exit, the inspector's fade when entering
  the grid from the viewer, toasts, smooth paging,
  progress and spinner loops. A scope change does not yet restore the previous
  scroll position as the Electron grid does.
- No glass surfaces or window vibrancy yet. In-app backdrop blur needs a
  renderer patch; two open-source implementations exist to port. GPUI cannot
  scale or rotate arbitrary elements, so the menu and modal scale-in needs a
  substitute.
- Sidebar expansion, rail widths, hidden rails and view changes are not
  persisted. The sidebar cannot be resized.
- The grid lays out every loaded item; the Electron grid's windowed model for
  very large libraries is not ported.
- Accessibility exposes the native window but not the control tree.
- The grid is still one element tree per tile, and never loads an original
  for a tile larger than its thumbnail. The media surface still carries
  colours of its own.
- No frame-time, decode-latency or memory benchmark has been completed. Do not
  call the prototype faster, flicker-free or memory-bounded on this evidence.
- Windows and Linux are not built or tested. The Ruffle frame path reads back
  from the GPU and uploads again; it is not zero-copy.

## Checks

```sh
cargo test --manifest-path experiments/gpui/Cargo.toml
PICTO_GPUI_TEST_DB=/tmp/picto-gpui-library.XXXXXX/library.sqlite \
  cargo test --manifest-path experiments/gpui/Cargo.toml -- --ignored
```

The regular tests cover the three layout modes against hand-computed geometry,
the viewer's fit, focal zoom, pan, glide and navigator arithmetic, the
logarithmic zoom track,
banded visibility against a full scan of 20,000 tiles in every mode, view
preference fallbacks, grid labels, popover placement, the known-library
order and name search, the reveal timeline, the easing curve
against the sampled bezier, tree ordering and the text-search gates. The
ignored tests need a real snapshot or a GPU and are run explicitly.

`pixel_diff.py` compares two captures of the same size using only the Python
standard library, and reports the share of differing pixels for the whole
image and for each named region, with a side-by-side image and a mask of where
they differ. Open both windows at the same size (`PICTO_GPUI_WINDOW=WxH`) with
the same content, scroll, selection and focus; do not stretch a capture to
make sizes agree. `--shift` moves the candidate first, which tells a uniform
offset from real differences.

On 13 November 2026, at 1213×865 with the same item selected in both apps,
4.4% of the window's pixels differed from the Electron capture by more than
16 levels: titlebar buttons 0.2%, toolbar 0.2%, library header 1.1%, system
rows 1.4%, section headers 2.2%, sidebar filter 1.6%, grid 5.2%, inspector
fields 0.9%, inspector chips 4.3%, and the window's edges none. The grid's
share is mostly originals the Electron grid loads for large tiles; part of
the rest is state that differed and JPEG noise in the captures.

`PICTO_GPUI_POINTER=<file or FIFO>` replays pointer events written to that
file (`move X Y`, `down X Y [clicks]`, `up X Y [clicks]`, `rdown X Y`,
`rup X Y`, `scroll X Y DISTANCE`, in window points), so hover, pressed,
tooltip, menu and navigation states can be checked without taking the real
pointer. GPUI
stops painting a window that is fully covered, so the window has to be
visible. `PICTO_GPUI_INSPECTOR=<width>` opens with the inspector at a stored
width, and `PICTO_GPUI_BACKGROUND=1` opens without taking keyboard focus.

`PICTO_GPUI_STATS=1` prints, for each second in which something happened, how
often each view repainted, how long the grid took to build a frame, the gap
between grid frames, and how many thumbnail files were read.

## Structure

One model, separate views, shared components:

- `session.rs` is the model: the destination, its results, the selection and
  the viewer. It talks to `backend.rs`, the small adapter to the existing Rust
  owners, and says what kind of thing changed.
- Each part of the window is its own cached view that repaints only for the
  changes it shows: `shell/` (the root, the sidebar, the library popover, the toolbar), `grid/`
  (the grid view with its layout, saved view, labels, reveal, keyboard
  movement and thumbnail cache), `inspector.rs` and `viewer/` (the media
  surface with its zoom and pan arithmetic in `camera.rs`, Flash, and the
  viewer toolbar the titlebar shows).
- `ui/` holds the shared components, one implementation each: icon button,
  tooltip, context menu, popover, library avatar, sidebar row, section
  header, chip, inspector section, action row,
  field frame and text input, scrollbar, "Return to Top", badge, zoom
  control and the titlebar drag region. Views compose these.
- `theme.rs` holds the design tokens, derived from one background, one
  contrast colour and one text colour as `tokens.css` derives them.
  `motion.rs` holds durations and easing. Views take colours, sizes and
  timings from these and write none of their own.

Production integration should reuse the application service layer for jobs
and events rather than growing a second application backend.

## Attribution

`ui/text_input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Sidebar icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Toolbar glyphs are Picto's own, from `src/shared/ui/icons/toolbar-icons.tsx`.
`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`.
