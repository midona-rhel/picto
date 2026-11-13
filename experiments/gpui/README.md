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
- Real paginated results (500 per page, loaded near the end), viewport-only
  tiles, bounded thumbnail cache (192 entries, four concurrent decodes).
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
- Space/Enter/double-click preview, Escape back, previous/next item, and
  navigation through members of a collection. Thumbnail-first preview with
  asynchronous full decode; stale loads are fenced.
- The Tags screen (`TagManagerScreen.tsx`): the 184px rail of All, General,
  Starred, the groups in their preset order and colours with their counts,
  "Create tag group" and "Delete Unused"; the tags of the chosen group in
  letter sections (or one "Matches" section while searching), as many
  columns as fit, with the group's dot, the star, the count and the section
  rules. Pages of 500 load as the list nears what is loaded, with shimmering
  placeholders for the rest. Changing group fades the list out over 100 ms
  and the new one in over 140 ms. The titlebar shows the tag search in place
  of the grid's controls; the library ranks the matches.
- The Tags screen's actions, through the library: rename (or merge into a
  tag of that name), merge into a chosen tag, delete, move to or out of a
  group, create, rename, recolour and delete groups, delete unused tags, star
  and unstar (kept in the library's application settings with the group
  colours), copy a tag's name, and "Filter Items with This Tag", which opens
  All filtered to the tag as a step of history.
- Dialogs as `GlassModal` draws them (`ui::modal`): backdrop, glass panel,
  title bar with its close button, body and footer, the 150 ms fade and rise
  in and 120 ms fade out, Escape and a click on the backdrop to close. The
  tag editor, merge picker, group dialogs and confirmations use it, with the
  shared action button (`actionButton.module.css`) and dialog fields.
- Context menus have submenus (open on hover, close 150 ms after the pointer
  leaves, arrow keys), a row of colour swatches, and the search field long
  menus get, which also finds submenu rows.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).

## Not yet equivalent to Picto

- Subscriptions and Duplicates are sidebar rows without screens.
- Tags screen: ⌘F does not move to its search; a failed save of the tag
  preferences shows in the screen's error banner, not as a toast; the editor's
  name field is dimmed while an edit runs but still takes typing; Undo does
  not reach tag edits; tag and group changes made elsewhere show when the
  screen is opened again, not live.
- Drawn with their hover states and tooltips but not wired: Settings, View
  options, the section headers' add buttons, the inspector's Add to Folder,
  Add Tags, Export, source-address list and Pin, and the library switcher.
  Each has a `TODO` where its action belongs. The Filter button only clears a
  filter the inspector set; there is no filter bar.
- With nothing selected the inspector is empty; the Electron one describes
  the scope. With several items selected it shows the last one, not the
  stacked summary.
- Not in the grid yet: rubber-band selection, dragging items out or to
  reorder, the hover preview of the original, broken-thumbnail, font and
  audio tiles, inline rename, and the scope-specific empty states.
- Not in the sidebar yet: the Subscriptions activity dot, smart-folder groups
  and custom folder icons and colours, rename in place, context menus,
  multi-selection and drag reorder.
- No video or audio playback, PDF, text, EPUB, CBZ, DjVu, JPEG XL or font
  viewers yet. Word and PowerPoint viewing is out of scope: those files import,
  keep their embedded thumbnail and open in the default application.
- Collection members use previous/next controls, not the continuous collection
  scroll surface. Preview is embedded, not the three production shells.
- Context menus exist only for grid items and the Tags screen, and are
  drawn inside the window, so they cannot extend past its edge. "Delete
  Permanently" is shown disabled: the prototype reads the library's files in
  place and must never delete them.
- No import/export, subscriptions, cloud, localization or site login.
  The viewer still has its own header instead of the titlebar's viewer
  toolbar.
- Motion still to port: the dialogs' and menus' scale (they fade and move
  but cannot scale), the inspector's fade when entering
  the grid from the viewer, toasts, the viewer's
  thumbnail-to-original crossfade and its zoom and pan tween, smooth paging,
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
- The grid is still one element tree per tile. The thumbnail cache has no
  cancellation or priority and never loads an original for a tile larger than
  its thumbnail. The media surface still carries colours of its own.
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
the Tags screen's sections, rows, ordering, search ranking, shimmer and stored
preferences, menu search and submenu placement, the dialogs' timing,
banded visibility against a full scan of 20,000 tiles in every mode, view
preference fallbacks, grid labels, the reveal timeline, the easing curve
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
  changes it shows: `shell/` (the root, the sidebar, the toolbar), `grid/`
  (the grid view with its layout, saved view, labels, reveal, keyboard
  movement and thumbnail cache), `inspector.rs`, `viewer/` (image and Flash
  presentation) and `screens/` (destinations that are not a grid, such as
  `screens/tags/`).
- `ui/` holds the shared components, one implementation each: icon button,
  tooltip, context menu, dialog, action button, sidebar row, section header, chip, inspector section, action row,
  field frame and text input, scrollbar, "Return to Top", badge, button, zoom
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
