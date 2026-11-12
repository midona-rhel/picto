# Picto GPUI browser MVP (#163)

An interactive native media-browser experiment, not an Electron replacement yet.
Issue: https://github.com/midona-rhel/picto/issues/163

## Run safely

```sh
experiments/gpui/run.sh /path/to/Main.library
```

The launcher uses SQLite's online backup to create a disposable database under
`/tmp/picto-gpui-library.*`. It reads existing thumbnails and originals without
copying or changing blobs. The app refuses databases without the snapshot marker.
Rating edits and undo/redo affect only that copy. The original library and the
main checkout are not modified. Keep the snapshot until the app is closed.

The generated macOS bundle is `experiments/gpui/target/Picto GPUI.app`.
The experiment uses its own Cargo workspace and lockfile, pinned to GPUI 0.2.2.
It now uses the locally available committed dev baseline `84d90552`, whose schema
matches current libraries, rather than the original older scaffold baseline.

## Implemented

- Existing `picto_library` query, details, smart-folder, rating and undo owners.
  Database work runs on one worker thread, never in the rendering callback.
- Real paginated library results (500 per page, automatic loading near the end),
  viewport-virtualized waterfall tiles, adjustable tile size, selection and navigation.
- Bounded thumbnail cache (192 entries, four concurrent background decodes).
  Full-original decoding also runs off the UI thread, only while preview is open.
- Bundled Roboto inspector font, centered icon controls, Books library avatar,
  and a right-click menu for Quick Look and copying names or file paths.
- SVG originals use GPUI's native SVG image loader, not a browser surface.
- All, Inbox, Untagged, Uncategorized, Trash, folders and smart folders, including
  hierarchy, nested indentation, collapse controls and counts.
- Native editable search with selection, clipboard and IME plumbing from GPUI's
  official input example. Fewer than three trimmed characters leave filtering off.
  Query generations prevent an old successful result replacing a newer query.
- Real metadata inspector: dimensions, MIME, size, media count, tags and notes.
- Rating editing on the snapshot; Cmd+Z undo and Cmd+Shift+Z redo.
- Space/Enter/double-click preview, Escape back, previous/next item, and navigation
  through members of a collection.
- Thumbnail-first preview with asynchronous decoding. The old presented frame is
  retained until the new thumbnail arrives. In-flight loads are cancelled/replaced
  and stale results fenced.
- Picto's resolved dark-theme colors, SF Pro Text, 13px body text, 27px sidebar
  rows, selection accent, and the same Tabler outline icon assets.

## Verification performed

On macOS, in the actual bundled app against the disposable snapshot:

- Library grid and inspector display real thumbnails and metadata.
- Native search accepts input and returns matching library results.
- Smart-folder clicks change the query and count.
- Tile-size controls change column density.
- Click selection and Space preview work; Escape returns to the grid.
- A multi-image collection advances between its members.
- Left/right navigation returns to the matching item and metadata.
- Sidebar disclosure collapses a subtree without changing the query.
- Rating click changes the persisted snapshot rating; Cmd+Z restores it.

Automated checks:

```sh
cargo test --offline --manifest-path experiments/gpui/Cargo.toml
PICTO_GPUI_TEST_DB=/tmp/picto-gpui-library.XXXXXX/library.sqlite \
  cargo test --offline --manifest-path experiments/gpui/Cargo.toml -- --ignored
```

Six regular tests pass, including indexed visibility against a full scan of 20,000
mixed-size tiles, stable pagination, tree ordering and the text-search gates.
The explicitly enabled real-snapshot integration test passes: query, details,
smart-folder counts, rating mutation, undo, redo, then restoring the original rating.
Build and formatting checks pass. GPUI's upstream `block 0.1.6` dependency emits
an existing future-compatibility warning.

## Not yet equivalent to Picto

This is a useful browser/editing slice, not visual/behavioral parity certification.
In particular:

- Collection members currently use previous/next controls, not Picto's continuous
  collection scroll surface. Preview is embedded, not all three production shells.
- No drag/drop, range/multi-selection, complete context menus, import/export, subscriptions,
  cloud UI, localization or browser authentication integration yet.
- Video playback and comprehensive media-format parity remain unimplemented.
- Hover states work, but complete animation/easing, glass effects, inspector font
  metrics and all production layout modes still need side-by-side verification.
- Sidebar expansion and layout preferences are not persisted across launches.
- Accessibility exposes the native window but not the full GPUI control tree.
- No frame-time, decode-latency or texture-memory benchmark has been completed.
  Do not call the prototype faster, flicker-free, or memory-bounded on this evidence.
- Windows and Linux are not tested.

## Architecture and next decision

All media paths must work on macOS, Windows and Linux. No WebKit surface is used.
Native Ruffle 0.5.0 uses wgpu offscreen rendering, mouse/keyboard forwarding and
optional CPAL audio. Word uses isolated LibreOffice-to-PDF conversion and Poppler
page rendering. Both paths were exercised in the actual macOS GPUI window using
the bundled fixtures. The SWF test also verifies its ActionScript rollover handler;
the Word test checks two rendered pages and temporary-directory cleanup.
These are not Windows/Linux packaging results or comprehensive format certification.
Audio is wired but not audibly verified. Flash external networking is not enabled.
The GPU readback/upload path is not zero-copy. Word requires `soffice` and `pdftoppm`
on PATH, or `PICTO_SOFFICE` and `PICTO_PDFTOPPM`; redistributing those runtimes is
still packaging work. Media Examples in the menu opens SVG, Flash and Word fixtures.

## Visual and performance checks

`pixel_diff.py` uses Pillow to produce an unscaled overlay, amplified RGB difference,
threshold mask and numeric report. It rejects unequal capture dimensions unless
explicit crops are provided. Compare equal content, scale, focus and scroll state;
do not stretch screenshots to conceal layout errors.

On November 7, the top-sidebar 600x440 physical-pixel comparison against installed
Picto changed from 15.51% to 8.11% differing pixels at a 16/255 channel threshold.
This is a **remaining mismatch**, not acceptance: snapshot counts and avatar crop
differ, and navigation/actions are incomplete. Captures are local under
`/tmp/picto-gpui-visual`, not committed library screenshots.

The standalone Cargo workspace now explicitly optimizes debug dependencies rather
than silently losing the parent workspace's decode optimizations. Visibility uses
a per-column index. `benches/visibility.rs` compares it with the previous full scan;
at 20,000 tiles the local lookup measured 538 ns versus 6978 ns. This measures only
geometry lookup, **not frame time**. Full-shell invalidation during scrolling still
needs separation, and smoothness/p95/p99 frame-time acceptance remains open.

`backend.rs` is the small adapter to existing Rust owners; it does not implement
SQL or duplicate ingestion business rules. `surface.rs` owns image presentation,
`input.rs` native text input, `theme.rs` copied visual tokens and icon assets, and
`main.rs` the prototype shell. Production integration should reuse the application
service layer for jobs/events rather than growing a second application backend.

Before a migration decision, compare both renderers on the same images and viewport:
p50/p95/p99 frame times, input latency, blank transition frames, cold launch, peak
memory and memory after repeated scrolling. Then prove media coverage, desktop
integration, authentication, accessibility and three-platform packaging separately.

GPUI's `runtime_shaders` feature avoids requiring a separately downloaded Metal
compiler for the experiment. Cold-launch shader compilation must be measured.

## Attribution

`input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Official references: https://gpui.rs/ and
https://github.com/zed-industries/zed/tree/main/crates/gpui/examples .

`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`. The SVG and Word fixtures are generated
test documents. Custom question-mark icons use Picto's existing sidebar SVG paths.

## Recovery checkpoint (November 12)

The uncommitted temporary checkout was lost. Source files were reconstructed from
recorded source-writing commands and patches, rather than decompiled from binaries.
Public icon/SWF assets were restored from their original packages; fonts and the
Word fixture were regenerated using the original recipes. Cargo.lock was regenerated
from available dependencies, so dependency resolution may differ from November 7.
The older visual/runtime observations above are historical evidence, not a claim
that every behavior was retested after recovery. This remains an unfinished experiment.
