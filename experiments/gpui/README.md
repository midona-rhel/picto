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
- Toolbar with Back and Forward over visited destinations, zoom, Picto's own
  16px glyphs, and native editable search (selection, clipboard, IME). Fewer
  than three trimmed characters leave filtering off. Query generations prevent
  an old result replacing a newer query.
- Real paginated results (500 per page, loaded near the end), viewport-only
  tiles, bounded thumbnail cache (192 entries, four concurrent decodes).
- Metadata inspector: dimensions, MIME, size, media count, tags, notes, rating.
- Space/Enter/double-click preview, Escape back, previous/next item, and
  navigation through members of a collection. Thumbnail-first preview with
  asynchronous full decode; stale loads are fenced.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).

## Not yet equivalent to Picto

- Tags, Subscriptions and Duplicates are sidebar rows without screens. The
  toolbar's view-options and filter buttons, and the sidebar's settings and
  panel buttons, are drawn but not wired.
- No video or audio playback, PDF, text, EPUB, CBZ, DjVu, JPEG XL or font
  viewers yet. Word and PowerPoint viewing is out of scope: those files import,
  keep their embedded thumbnail and open in the default application.
- Collection members use previous/next controls, not the continuous collection
  scroll surface. Preview is embedded, not the three production shells.
- No drag and drop, range or multi-selection, complete context menus,
  import/export, subscriptions, cloud, localization or site login.
- No animations, glass surfaces or window vibrancy yet. In-app backdrop blur
  needs a renderer patch; two open-source implementations exist to port.
- Sidebar expansion, sidebar width and view changes are not persisted.
- The grid lays out every loaded item; the Electron grid's windowed model for
  very large libraries is not ported.
- Accessibility exposes the native window but not the control tree.
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
preference fallbacks, grid labels, tree ordering and the text-search gates. The
ignored tests need a real snapshot or a GPU and are run explicitly.

`pixel_diff.py` (needs Pillow) produces an unscaled overlay, amplified RGB
difference, threshold mask and numeric report for two equal-size captures.
Compare equal content, scale, focus and scroll state; do not stretch screenshots
to conceal layout errors.

## Structure

`backend.rs` is the small adapter to the existing Rust owners; it does not
implement SQL or duplicate ingestion rules. `layout.rs` is the grid geometry,
`prefs.rs` the saved view, `format.rs` the grid labels, `shell.rs` the sidebar
and toolbar, `inspector.rs` the inspector, `surface.rs` image and Flash
presentation, `input.rs` native text input, `theme.rs` design tokens and icons,
and `main.rs` the window and grid. Production integration should reuse the
application service layer for jobs and events rather than growing a second
application backend.

## Attribution

`input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Sidebar icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Toolbar glyphs are Picto's own, from `src/shared/ui/icons/toolbar-icons.tsx`.
`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`.
