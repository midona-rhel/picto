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
  rect, optional name and resolution rows.
- Sidebar with the Electron row order (All, Inbox, Recently Viewed,
  Uncategorized, Untagged, Tags, Random, Subscriptions, Duplicates, Trash),
  Folders and Smart Folders sections with counts, hierarchy, collapse controls
  and a filter. Random shuffles the All scope with one seed per session.
- The Electron shell's layout: a 48px titlebar row whose left block keeps the
  sidebar's width, over a body whose rails slide away (sidebar 50 ms,
  inspector 100 ms) while the grid takes the freed width at once. The panel
  button and Tab toggle both rails; Cmd+Alt+1 and Cmd+Alt+2 toggle one.
- The app owns titlebar dragging on macOS. The titlebar strips move the
  window and controls inside them do not, as the Electron shell's drag
  regions work.
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
- Metadata inspector: dimensions, MIME, size, media count, tags, notes, rating.
- Space/Enter/double-click preview, Escape back, previous/next item, and
  navigation through members of a collection. Thumbnail-first preview with
  asynchronous full decode; stale loads are fenced.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).

## Not yet equivalent to Picto

- Tags, Subscriptions and Duplicates are sidebar rows without screens. The
  toolbar's view-options and filter buttons and the settings button are drawn
  but not wired.
- No video or audio playback, PDF, text, EPUB, CBZ, DjVu, JPEG XL or font
  viewers yet. Word and PowerPoint viewing is out of scope: those files import,
  keep their embedded thumbnail and open in the default application.
- Collection members use previous/next controls, not the continuous collection
  scroll surface. Preview is embedded, not the three production shells.
- No drag and drop, range or multi-selection, complete context menus,
  import/export, subscriptions, cloud, localization or site login.
- Motion still to port: modal enter/exit, the inspector's fade when entering
  the grid from the viewer, toasts, the viewer's
  thumbnail-to-original crossfade and its zoom and pan tween, smooth paging,
  progress and spinner loops. A scope change does not yet restore the previous
  scroll position as the Electron grid does.
- No glass surfaces or window vibrancy yet. In-app backdrop blur needs a
  renderer patch; two open-source implementations exist to port. GPUI cannot
  scale or rotate arbitrary elements, so the menu and modal scale-in needs a
  substitute.
- Sidebar expansion, sidebar width, hidden rails and view changes are not
  persisted. The rails cannot be resized.
- The grid lays out every loaded item; the Electron grid's windowed model for
  very large libraries is not ported.
- Accessibility exposes the native window but not the control tree.
- The grid is still one element tree per tile, the thumbnail cache has no
  cancellation or priority, and the text input and media surface still carry
  colours of their own.
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
banded visibility against a full scan of 20,000 tiles in every mode, view
preference fallbacks, grid labels, the reveal timeline, the easing curve
against the sampled bezier, tree ordering and the text-search gates. The
ignored tests need a real snapshot or a GPU and are run explicitly.

`pixel_diff.py` (needs Pillow) produces an unscaled overlay, amplified RGB
difference, threshold mask and numeric report for two equal-size captures.
Compare equal content, scale, focus and scroll state; do not stretch screenshots
to conceal layout errors.

## Structure

One model, separate views, shared components:

- `session.rs` is the model: the destination, its results, the selection and
  the viewer. It talks to `backend.rs`, the small adapter to the existing Rust
  owners, and says what kind of thing changed.
- Each part of the window is its own cached view that repaints only for the
  changes it shows: `shell/` (the root, the sidebar, the toolbar), `grid/`
  (the grid view with its layout, saved view, labels, reveal, keyboard
  movement and thumbnail cache), `inspector.rs` and `viewer/` (image and Flash
  presentation).
- `ui/` holds the shared components, one implementation each: icon button,
  sidebar row, section header, badge, button, text input, zoom control and the
  titlebar drag region. Views compose these.
- `theme.rs` holds the design tokens, derived from one background, one
  contrast colour and one text colour as `tokens.css` derives them.
  `motion.rs` holds durations and easing. Views take colours, sizes and
  timings from these and write none of their own.

Production integration should reuse the application service layer for jobs
and events rather than growing a second application backend.

## Attribution

`input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Sidebar icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Toolbar glyphs are Picto's own, from `src/shared/ui/icons/toolbar-icons.tsx`.
`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`.
