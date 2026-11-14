# Picto GPUI frontend

## What this is

A native frontend for Picto written in Rust on GPUI (the UI framework of the
Zed editor), built to behave and look like the Electron app's renderer
(`src/` at the repository root). It runs Picto's own application layer
(`core/`, `picto-library/`) inside the process, as the Electron app does
through its native addon, so commands, background workers and events are
production's. It covers most of what a user does day to day (browsing,
selecting, the inspector, menus, the viewers, imports, exports, the Tags,
Subscriptions and Duplicates screens, Settings), but it is not a replacement:
section 8 lists what is missing and what has never been checked. It opens
only a disposable copy of a library, never a library the user keeps. It is
developed and used on macOS; Windows and Linux are built, tested and
photographed off-screen in CI and have never been used on a screen.
Issue: https://github.com/midona-rhel/picto/issues/163

## Running it safely

### The safety boundary

The app writes, imports and deletes files, so it opens only a folder it can
prove is a throwaway copy (`application::guard` in `src/application.rs`). A
copy is accepted only when all of these hold:

- it holds the marker file `.gpui-disposable`, whose first line names the
  library it was copied from (or `synthetic:…` for a generated library);
- it is not that library, not inside it, and does not contain it;
- it sits exactly one level inside a folder named `picto-gpui-…` that lies
  directly in a temporary folder: the system's (`std::env::temp_dir()`, which
  is `$TMPDIR` on macOS and `%TEMP%` on Windows) or, on macOS and Linux,
  `/tmp` or `/var/tmp`. A marked folder moved or copied anywhere else is
  refused, because a marker travels with the files;
- none of the copy's top-level entries, none of the entries of `blobs/`, and
  neither `blobs/f` nor `blobs/t` is a symbolic link (what is written through
  a link lands where it points);
- its `library.sqlite` is not the source's database reached another way (a
  hard link).

Before the application starts its workers, and again on every launch,
`application::detach` cuts the copy loose from work the real library has
going on. In one transaction it:

- disconnects cloud sync (provider, account and remote root cleared, state
  disabled), so nothing is written to the user's cloud folder;
- switches off every watched folder;
- clears every subscription's next run, and cancels subscription runs and
  run queries that were pending or running; choosing a schedule again in the
  app sets a next run;
- clears "delete after import" on every import job, so no job removes a file
  outside the copy once it has been imported.

On the first open only (it then writes `.gpui-detached` in the copy) it also
marks import jobs still pending or running as failed ("Queued in the library
this copy was made from"), and rewrites every stored `media_file.file_path`
that starts with the source library's path to start with the copy's, so that
nothing opened, revealed, copied or dragged out is a file of the real
library.

The application data the core keeps outside a library (AI models, caches) is
pointed at `app-data/` beside the copy, never at the Electron app's folder.

Files are handed to the rest of the desktop only through `src/desktop/`
(open with the default application, Open With, reveal in the file manager,
copy files, Copy File Path, drag out of the window). Each canonicalises the
path and refuses, with a notification, any file that is not inside the open
copy, whatever the library stored. Links are opened only when they are
`http:` or `https:` (`desktop::open_link`).

Delete Permanently, Empty Trash and the Duplicates screen's Keep and Smart
merge therefore remove files from the copy's blobs only. The source library
is only ever read, to copy it.

### Making a copy and opening it

On macOS, `run.sh` copies a library, builds the app and opens it:

```sh
experiments/gpui/run.sh /path/to/Main.library
```

It copies the database with SQLite's online backup from a read-only
connection (`sqlite3 -readonly … .backup`, which includes committed WAL data
and never touches the live files), then clones everything else in the library
folder with APFS `clonefile` (`cp -Rc`), which takes no space until either
side changes a file. It refuses, copying nothing, when the library and `/tmp`
are on different volumes or the volume is not APFS. The copy goes to
`/tmp/picto-gpui-library.XXXXXX/<Name>.library`. It then builds the debug
binary, wraps it as `experiments/gpui/target/Picto GPUI.app` (bundle id
`dev.picto.gpui-experiment`; the bundle is needed for system notifications),
writes the copy's path to `target/last-snapshot` and opens the bundle on the
copy. It needs the `sqlite3` command. It does not look for symbolic links;
the app refuses such a copy when it opens it. `PICTO_GPUI_COPY_ONLY=1` makes
the copy, prints its path and stops.

On every platform the app makes the copy itself:

```sh
cd experiments/gpui
cargo run --locked -- --make-copy /path/to/Main.library   # prints the copy's path
cargo run --locked -- <that path>
```

`--make-copy` (`application::make_copy`) puts the copy in a new
`picto-gpui-library.…` folder of the system's temporary folder, backs up the
database the same way, copies everything else with the system's file copy
(which clones on APFS, btrfs, XFS and ReFS and copies in full elsewhere, so a
large library needs that much free space there), refuses a library that holds
a symbolic link anywhere, writes the marker, detaches the copy and checks it
with the guard. A failed copy leaves nothing behind. Run the binary through
`cargo run` on Linux: it links the core's ONNX Runtime libraries
(`libwebgpu_dawn.so` among them), which cargo puts on the loader's path and a
bare `target/debug/picto-gpui-spike` does not find.

The binary takes the copy's root first. `<copy> --detach` detaches the copy
and exits without opening a window; `reference.sh` uses it before the
Electron app opens the same copy. Files after the copy are opened as
production opens files from the file manager (`src/opening/`): a
`.picto-pack` goes to the pack's import dialog (several wait their turn),
a `.library` that is the open copy only brings the window forward, and any
other `.library` is refused, since the app opens only the copy it started
on. There is one instance per copy: a second launch on a copy already open
hands its files to that instance, which comes forward, and exits; launches
on other copies run side by side. A launch with files and no copy (what a
file manager starts) hands them to the instance whose window was active
last, or fails when none is open. On macOS the same files also arrive
through `application:openURLs:` (Open With, `open -a`, a drop on the Dock
icon) while the app runs. No file type is registered with the system: the
bundle `run.sh` makes declares none, and nothing is written to the
registry or to a `.desktop` file.

`million.sh [items]` generates a synthetic library (a million items by
default, sharing 4,096 generated JPEGs through hard links, with five smart
folders) with the core's `million_grid_demo` example, built in release mode
from the repository root, in `/tmp/picto-gpui-synthetic.XXXXXX/`, marks it
`synthetic:million_grid_demo`, builds the app and opens it with the frame
meter on. It reads no existing library. `PICTO_GPUI_MILLION_ONLY=1` makes the
library, prints its path and stops.

Delete copies when done: `/tmp/picto-gpui-*` and `$TMPDIR/picto-gpui-*` on
macOS and Linux, `%TEMP%\picto-gpui-*` on Windows. A copy made by
`reference.sh` also holds the Electron app's data in `electron-data/` beside
it.

### Frame meter and counters

Every build reads these two variables; with neither set nothing is added.

`PICTO_GPUI_FPS=1` (`src/meter.rs`) shows a meter in the window's bottom-left
corner: frames drawn in the last second, the time between the last two, and
how long the app spent drawing one, from the start of the shell's render to
the end of its paint (the GPU's work afterwards is not in it). It reads
"idle" once nothing has drawn for a second, and asks for no frames of its own
beyond one a second to say so.

`PICTO_GPUI_STATS=1` (`src/stats.rs`) prints to standard error, for each
second in which something happened: how often each view repainted; how long
the grid took to build a frame, by the clock and in the main thread's own
time, with its layout and tile parts; the gap between the window's frames;
the longest a 4 ms timer on the main thread was kept waiting (a stall); how
many thumbnail files were read, let go, let go without ever being painted, or
dropped unfinished; the most bytes the decoded thumbnails held; and how many
pictures were released from GPUI's sprite atlas (in the off-screen tool, also
how many an atlas still held afterwards, which should be none). It also
prints at once each stage of opening (database readable, application
finished), each window of results read and when it arrived, and every screen
read or edit on the library thread that took 5 ms or more. Unset, the timings
read no clock and each counter costs one atomic add.

## Building

The experiment is a Cargo workspace of its own (`experiments/gpui/Cargo.toml`
and `Cargo.lock`), outside the production workspace. The toolchain is Rust
1.98.0, pinned by the repository's `rust-toolchain.toml`. Without rustup,
install that toolchain anywhere and put its `bin` first on `PATH`;
`experiments/gpui/.toolchain/` is ignored by Git for that purpose.

```sh
cd experiments/gpui
cargo build --locked     # debug, as run.sh, million.sh, reference.sh and CI build
cargo test --locked
```

GPUI is taken from `gpui-pre` and `gpui-pre-platform` `=0.3.12`, a crates.io
snapshot of upstream GPUI (zed-industries/zed); the official `gpui` crate has
not been published since 0.2.2 and GPUI promises no API stability between
snapshots, so the version is pinned exactly and upgraded deliberately. Four
of the snapshot's crates (`gpui-pre`, `gpui-pre-apple`, `gpui-pre-wgpu`,
`gpui-pre-windows`) are vendored under `vendor/` and substituted with
`[patch.crates-io]`. The patches add a backdrop-blur primitive (drawn by the
Metal and wgpu renderers, ignored by the DirectX one), drop shadows that cut
their element out as CSS does, nearest-neighbour image sampling for the
"Pixelated" preview setting, a native file drag the app can start itself
(`Window::start_platform_drag`), and test-only hooks for the off-screen
checks (`Window::is_dirty`, `Window::has_image_atlas_entry`, test windows
that keep client decorations). `vendor/README.md` explains each change and
holds the full diff against 0.3.12, so it can be re-applied to a newer
snapshot.

The `dev` profile builds this crate at `opt-level = 1` and every dependency
at `opt-level = 2`, with line-number debug information only and incremental
compilation off, because full debug information and incremental caches made
each build cycle write several gigabytes. There is no tuned release profile;
`cargo build --release` drops the test seams (section 5).

Expect a large build: the lockfile holds about 1,100 packages (the
application core, ONNX Runtime, Ruffle, wgpu and the document decoders among
them), the debug binary is about 156 MB, and one worktree's `target/` folder
measured about 7 GB after debug builds and tests. `million.sh` also builds the
core's example in release mode in the repository root's `target/`. A clean
build takes several minutes; it has not been timed with the current
dependencies.

Platforms: macOS is where the app is developed and the only platform it has
run on with a screen. Windows and Linux are built and tested in CI (section
6). Linux needs the packages CI installs: `pkg-config clang cmake
libasound2-dev libclang-dev libdbus-1-dev libfontconfig-dev libglib2.0-dev
libssl-dev libvulkan1 libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev
libzstd-dev libudev-dev` (and `mesa-vulkan-drivers` for software drawing).
Video other than what AVFoundation plays needs the `ffmpeg` binary the core
already uses, found through `PICTO_FFMPEG_DIR`, then `vendor/ffmpeg/` in or
above the working directory or the executable
(`scripts/download-ffmpeg.sh` fetches it), then `PATH`.

## How it is put together

About 112,000 lines of Rust in `src/`. The map below says what each area
owns, its main types, and the one rule to know there.

**Application and library thread** (`application.rs`, `backend.rs`).
`Application` runs `picto_core` on a Tokio runtime of its own: it sets the
application data root, registers the core's event callback, opens the copy
with `picto_core::state::open_library_reporting` and answers
`dispatch("command", json)` exactly as the renderer's `invoke` is answered
(`core/src/ipc.rs`). `backend::start` runs one library thread that borrows
the application's own `Library`, so the database is opened once; grid
results and the sidebar are read as soon as the database is readable, while
the core's recovery passes still run (10–30 s on a million items), and
everything else waits for the whole open, as commands do. Rule: the library
is never touched from the thread that draws.

**Session** (`src/session/`). `Session` is the one model every view reads:
the destination list (`backend::Navigation`), the destination shown and the
history of places (`navigation.rs`), its results as a window over the
destination (`results.rs`), the selection (`selection.rs`), the viewer, and
library edits (`edits.rs`). It emits a `Change` (`Navigation`, `Results`,
`Selection`, `Details`, `Edited`, `Viewer`, `Activity`) so each view repaints
only for what it shows. `events.rs` drains the core's `library/changed` and
`picto:thumbnail-changed` events, merged into one per poll as the renderer's
registry merges them per frame, and does what production's settle stores do:
the sidebar's destinations and counts are read again, the shown grid is read
again in place (coalesced, spaced between 40 ms and 1 s by three times the
last refresh's time, held during a press, rubber band or drag or a change of
destination, keeping the selection by item and the topmost visible tile
where it was), and subscription activity is polled every 1.5 s while a run
goes. Rule: views read the session and change it through its methods; no
view owns another's state.

**Shared UI layer** (`src/ui/`). One implementation of each control, as
`src/shared/ui` is for production: buttons, icon buttons, check boxes and
radios, switches, selects, sliders, number fields, date and colour pickers,
chips, rows, the scrollbar, tooltips, empty states, segmented controls and
tab strips, breadcrumbs, window chrome, and the track-drag gesture and hover
fade they share. A control that only one view has in production (its own CSS
class, such as the grid's Retry button) is drawn by that view. The pieces
every view relies on:

- *Layers* (`ui/layer.rs`): the one drawing order for everything that floats,
  production's `--layer-*` scale by name (`OVERLAY`, `PANEL`, `DIALOG`,
  `MENU`, `POPOVER`, `TOOLTIP`, `DRAG`, `PREVIEW`, `NOTIFICATION`, …). GPUI
  draws all deferred elements of a frame in one list sorted by priority,
  however nested, so something that opens inside another (a tooltip in a
  dialog) needs the higher layer.
- *Floating panels* (`ui/popover.rs`): one placement rule (`place`) and one
  element (`Floating`) for everything laid beside the control that opened it
  (the library switcher, calendars, select lists, tag suggestions, the
  smart-folder dialog's popovers, the inspector's sources, the pickers),
  with its backdrop or outside press, Escape, and a Tab that stays inside.
  The pickers' movable panel is `ui/overlay_shell.rs`.
- *Focus* (`ui/focus.rs`): every shared control is a tab stop; a dialog, a
  popover and the Settings window are scopes Tab cycles inside; the ring
  shows only when Tab put the keyboard there.
- *Menus* (`ui/menu.rs`, `ui/menu/`): the context menu with submenus, check
  rows, swatch rows, embedded views and the search row. `presentation()`
  alone decides how a menu is shown: on macOS each panel is a pop-up window
  of its own that can reach past the main window; elsewhere, and with
  `PICTO_GPUI_MENU=window`, panels are drawn in the window.
- *Dialogs* (`ui/modal.rs`, `ui/confirm.rs`, `ui/form.rs`): the centred glass
  dialog with its backdrop and motion, the confirmation production calls
  `ConfirmModal`, and the form pieces.
- *Text input* (`ui/text_input/`): one `TextInput` with builder options,
  split into keys per platform, editing commands, pointer, input method,
  layout and pure editing rules. It edits as Chromium does on each platform.
- *Notifications* (`ui/notify.rs`): `notify::error`, `warning`, `info` and
  `success` from any `cx`; the main window's `Host` draws one at a time above
  everything.
- *Glass* (`ui/glass.rs`): the surface of every floating panel and its
  backdrop blur.

**Theme, motion, keymap** (`theme.rs`, `motion.rs`, `keymap.rs`,
`settings/registry.rs`). `theme.rs` derives every colour from a theme's
background, contrast and text colours as `tokens.css` does (Auto, Light,
Light Gray, Gray, Dark, Blue, Purple) and holds sizes and fonts; `motion.rs`
holds durations, easings and the fade helpers. `settings/registry.rs` is
production's shortcut registry (`shortcuts.ts`) as data; `keymap.rs` turns it,
with the keyboard preset and the user's overrides, into bindings in the key
context of production's scope (`Anywhere`, `App`, `Grid`, `Viewer`,
`GroupEditor`, `Duplicates`, `Subscriptions`, `DetailWindow`, `Media`,
`TextField`), rebuilt when Settings applies a change. Nothing fires while a
text field, menu, popover or dialog has the keyboard. Rule: views take their
colours and timings from these files; only colour tables that are data (the
theme swatches, the colour editor's presets, a chip's label shades) sit with
the code that lists them, and a size that belongs to one component stays with
it, beside the CSS rule it ports.

**Grid** (`src/grid/`). `layout.rs` ports `layoutMath.ts` (waterfall, grid,
justified) in double precision. `window.rs` is the windowed model of
production's `gridWindow.ts`: at most 1,500 items are held, read at an exact
ordinal of the destination's order (first 500 at the top, then aligned to
500); the scroll height stands for every item (estimated from the first
items laid out, capped at 8,000,000 px), and items outside the window are
neither laid out nor drawn. `selection.rs` keeps the selection by item: items
chosen one by one, Select All less exceptions, and shift ranges as spans of
the order, which edits address through the library
(`SelectionTarget::Query`, `::Range`) and never through what is loaded.
`tile.rs` draws a tile (shared with the collection editor); `thumbnails.rs`
loads thumbnails six at a time in `thumbnailPlan.ts`'s order and keeps
decoded ones up to a byte budget of three windows of the smallest tiles;
`originals.rs` decodes originals for tiles larger than their 512 px
thumbnail; `rendered.rs` draws the thumbnails the core cannot make (PDF,
DjVu and comic first pages, Flash frames), only in a marked copy. Also here:
the context-menu rows (`menu.rs`, pure functions), filters and the filter
bar, rubber band, keyboard movement, hover preview, subfolder tiles, drag
out. Rule: every decoded picture leaves GPUI's sprite atlas through
`crate::atlas::release`.

**Viewer** (`src/viewer/`). One media surface (`surface.rs`) shows any item:
images with zoom and pan (`camera.rs`), animated images, Flash through Ruffle
(`flash.rs`), video and audio in the media player (`video/`), and documents
(`document/`). It has three hosts: the media view that replaces the grid
(`mod.rs`, with `toolbar.rs` in the titlebar), Quick Look over the whole
window (`quick_look.rs`), and detail windows (`detail.rs`); `trigger.rs`
decides which a key or click opens. `document/kind.rs` chooses the renderer
for a media type as `detailRendererKind.ts` does; `document/paged.rs` shows
PDF (hayro), DjVu (djvu-rs) and comic archives as one continuous column;
`document/reader.rs` and `rich.rs` show text, Markdown, JSON, RTF and EPUB;
`document/font.rs` the font viewer. The player sees media only through
`video/backend.rs`: AVFoundation on macOS for QuickTime and MPEG-4 (frames
drawn without copying), the bundled `ffmpeg` binary for everything else,
Symphonia and CPAL for sound. Collections open in the reader (`group.rs`) or
the editor (`group_editor.rs`); `review.rs` is the Inbox's Accept and Reject
pill. Rule: decoding never runs on the thread that draws.

**Screens** (`src/screens/`). Destinations that are screens rather than a
grid: Tags (`tags/`), Subscriptions (`subscriptions/`), Duplicates
(`duplicates/`). A screen can hand the titlebar its own controls. The tag
browser model the Tags screen, the tag picker and the Add Tags dialog share
is `src/tags.rs`; the folder tree the folder picker and the Folders dialog
share is `inspector/folder_picker.rs`. The shared cover dialog
(`MediaCoverDialog`) lives in `screens/subscriptions/cover_dialog.rs` and is
also used for the library cover.

**Inspector** (`src/inspector/`). The right rail: the destination when
nothing is selected (`scope.rs`), one item (`mod.rs`), several
(`selection.rs`), the tag and folder pickers and their dialogs, sources,
colours, membership chips.

**Shell** (`src/shell/`). The window as `AppShell.tsx` arranges it (`mod.rs`):
titlebar, sidebar (`sidebar.rs`, `sidebar/`), rails and their resize edges
(`rails.rs`), toolbar, view menu, imports and exports from the window, batch
rename, permanent delete (`delete.rs`), the Inbox review wiring. Item menus
(`item_menu.rs`, `item_menu/actions.rs`) describe what was right-clicked and
perform the chosen row; keyboard shortcuts and application menu rows run the
same rows rather than their own edits. `menu_commands.rs` handles the
application menu's commands on the window, each only while production's rule
enables it; `GridScope` there is the one classification of the grid's
destination (kind, open folder, whether imports and drops are allowed) that
the application menu, item menus, keys and imports all use.

**Application menu** (`app_menu.rs`). One portable model of production's
`menu.mjs` (rows, titles, order, separators, keys from the registry, and a
`Platform` of Mac, Windows or Linux) with two presentations: the macOS menu
bar, and off macOS (or with `PICTO_GPUI_APP_MENU=window`) a titlebar button
that opens the same rows in the shared context menu. A row is enabled
exactly when something handles its action where the keyboard is; rows for
features not built are marked `.todo()` and show disabled.

**Imports, exports, dialogs** (`src/imports/`, `src/export/`, `prompt.rs`).
Imports go through the core's queue (`imports.enqueue`) with the folder and
collection questions and the task dialog and task bar; exports through
`media.export` and `picto_pack.*`. `prompt.rs` opens the system's file,
folder and save dialogs.

**Opening files** (`src/opening/`). The command line (`parse`: the copy,
then files), production's classification of opened files
(`associatedFileService.mjs`: `.picto-pack` and `.library`, made absolute
against the launch's working folder), and `accept`, which queues packs on
the export view's pack dialog and brings the window forward.
`opening/instance/` keeps one instance per copy: an exclusive lock on a file
named after the copy's identity in a folder of the user's own, and a Unix
socket (`unix.rs`) or named pipe (`windows.rs`) beside it that takes a later
launch's working folder and arguments as one JSON line.

**Desktop** (`src/desktop/`). What production's main process does around the
window: the Inbox badge, the "Subscriptions finished" system notification,
and the guarded file hand-offs. `mod.rs` is portable; `macos.rs`,
`windows.rs` and `linux.rs` hold each platform's calls.

**Settings and state** (`src/settings/`, `state.rs`). One settings model
loaded from the library's `setting` row `application` and `view_pref` row
`grid:defaults` with production's per-field fallbacks; the Settings window
edits a draft and Save & Close or Apply write it back. What production keeps
per machine (window bounds, rail widths and visibility, folded sidebar
sections, filter bar pins and saved filters, the tag picker's layout, recent
tags and folders, keyboard preset and rebound shortcuts, language, audio
visualization) is kept in `state.json` in the platform's configuration folder
under `Picto GPUI` (`~/Library/Application Support`, `%APPDATA%`,
`$XDG_CONFIG_HOME` or `~/.config`), never in the Electron app's folder or the
copy. It is written 250 ms after the last change through a renamed temporary
file and at once on quit; a field that cannot be read takes its default, an
unreadable file is set aside as `state.json.corrupt`, and fields of a newer
version are kept. The app starts on All each launch, as production does.

## Rules of the codebase

- **One implementation of each shared thing.** Before writing a control, a
  placement, a menu, a dialog, a tag or folder list, or a notification, use
  the one in section 4: floating panels `ui/popover.rs`, drawing order
  `ui/layer.rs`, focus `ui/focus.rs`, menus `ui/menu.rs`, dialogs
  `ui/modal.rs` and `ui/confirm.rs`, text fields `ui/text_input/`,
  notifications `ui/notify.rs`, tags `tags.rs`, the folder tree
  `inspector/folder_picker.rs`, the grid's destination `GridScope`, item
  actions `shell/item_menu/actions.rs`. Fix the shared one rather than adding
  a variant beside it.
- **Library edits** go through `session::edit_then` (or `edit_then_in` when
  the follow-up needs the window) or `Session::edit_reporting`, with a
  `Report` (the failure's title as production words it, a broad change
  announced with Undo) and a `Reread` (what to read again: details, the
  selection's summary, results in place, the destination list). Reads a
  screen needs go through `Session::run`, commands through
  `Session::dispatch`.
- **A view that changes state while a frame is drawn** (in `render`,
  `prepaint` or `paint`) and notifies must also call
  `window.request_animation_frame()`. Otherwise nothing schedules the next
  frame and the view looks stuck until the pointer moves. The off-screen
  tool's lazy mode exists to catch this (section 6).
- **Pictures** the app decodes leave the sprite atlas through
  `crate::atlas::release`, which defers the drop so it reaches every window.
- **Layers by name** from `ui/layer.rs`; no literal priorities.
- **Shortcuts** come from the registry through `keymap.rs`; a key that is
  not in the registry but differs by platform is bound with
  `keymap::bind_written`. Every registry id is either bound or listed with
  its reason in `keymap::NOT_YET`, and a test keeps it so.
- **Platform code** lives only in the platform leaves: `src/desktop/macos.rs`,
  `windows.rs` and `linux.rs` for the desktop hand-offs,
  `src/opening/instance/unix.rs` and `windows.rs` for the single-instance
  socket and pipe, and
  `src/platform/macos.rs` and `windows.rs` for window-level calls (AppKit and
  Core Animation for the pop-up menus, the traffic lights, the aspect lock,
  the window level; `SetWindowPos` on Windows), each reached through one
  shared function in `ui/window_chrome.rs`. All three platforms are required: a feature that
  works on one and not the others is listed in section 8 with what is
  missing.
- **Test seams are debug-build only.** Environment variables that drive or
  observe the app are read in `src/seams.rs` under `cfg(debug_assertions)`;
  a release build does not hold their names. The off-screen tool's
  variables, its dialog answers (`prompt.rs`) and the record of desktop
  requests (`desktop/`) exist in test builds only.
- **Comments describe behaviour**: no ticket numbers. A missing behaviour is
  a `TODO:` saying what is missing, and a `TODO` is never removed until the
  behaviour exists.

## Verifying a change

### Tests

```sh
cd experiments/gpui
cargo test --locked
```

About 600 tests, most of them pure: layout geometry against hand-computed
values, the grid's window and banded visibility (including
`a_frame_does_the_same_work_whatever_the_library_size`, at 3,000, a million
and fifty million items), the context menus against the cases of production's
`gridContextMenu.test.tsx`, the smart-folder rule model and the filter model
compiled to the library's `FilterExpr` and back, the keymap and text-field
keys per platform, the viewers' arithmetic, the media player's model and
audio processing, the safety guard and detach, the state file, and the
format matrix's fixtures.

The ignored tests need something a plain run does not have. Run them by
name:

| Test | Needs |
| --- | --- |
| `real_copy_answers_commands_and_reports_changes`, `real_snapshot_query_details_rating_undo_redo`, `real_snapshot_subscriptions_read`, `real_snapshot_tags_browse_rename_and_groups` | `PICTO_GPUI_TEST_LIBRARY` naming a copy; they write to it |
| `window_reads_stay_flat_across_the_library`, `time_each_stage` | the same; they time every grid read and each stage of opening (use `--nocapture`) |
| `offscreen::plays_a_script` | `PICTO_GPUI_COPY`, `PICTO_GPUI_SCRIPT`, `PICTO_GPUI_OUT` (below) |
| `viewers_render_off_screen` | a GPU; pictures saved when `PICTO_GPUI_MATRIX_OUT` is set |
| `renders_a_real_file` | `PICTO_GPUI_MEDIA=<file>` (type from the extension, or `PICTO_GPUI_MEDIA_MIME`) |
| `stop_reloads_the_movie_at_its_first_frame`, `real_swf_produces_frames_and_responds_to_mouse` | a native GPU adapter (Flash) |
| `real_webm_produces_frames` | `PICTO_GPUI_TEST_WEBM` and the `ffmpeg` binary |
| `decodes_and_seeks_a_real_audio_file` | `PICTO_GPUI_TEST_AUDIO` |
| `plays_through_the_output_device` | `PICTO_GPUI_TEST_AUDIO` and a sound device |

```sh
PICTO_GPUI_TEST_LIBRARY="$(PICTO_GPUI_COPY_ONLY=1 ./run.sh /path/to/Main.library)" \
  cargo test --locked -- --ignored --test-threads=1 --nocapture \
  real_copy real_snapshot window_reads_stay_flat time_each_stage
PICTO_GPUI_TEST_WEBM=/path/to/video.webm PICTO_GPUI_TEST_AUDIO=/path/to/audio.mp3 \
  cargo test --locked -- --ignored real_webm decodes_and_seeks plays_through
PICTO_GPUI_TEST_VIDEO=/path/to/video.mov cargo test --locked --test avfoundation_frames
```

The last is a plain program (`tests/avfoundation_frames.rs`), since
AVFoundation reports to the main thread, which the test harness does not give
a test; it skips without the variable. It checks that frames arrive as the
buffers GPUI draws, at the item's size, with the BT.601 matrix, at advancing
times, and that a seek and a paused picture arrive.
`real_copy_answers_commands_and_reports_changes` also resolves a duplicate
pair and checks the source library's blob is unchanged.

### The off-screen tool

`offscreen::plays_a_script` (`src/offscreen.rs`) opens the whole app on a
copy in a headless window, plays a script and saves a PNG per `shot`. On
macOS it draws with Metal and the system's text; on Linux and Windows with
GPUI's wgpu renderer (a software adapter where there is no GPU: lavapipe on
Linux, WARP through Direct3D 12 on Windows) and a text system holding only
the bundled fonts.

```sh
cd experiments/gpui
PICTO_GPUI_SILENT=1 PICTO_GPUI_MENU=window \
PICTO_GPUI_COPY=/tmp/picto-gpui-library.XXXXXX/Name.library \
PICTO_GPUI_SCRIPT=steps.txt PICTO_GPUI_OUT=shots \
  cargo test --locked -- --ignored --exact offscreen::plays_a_script --nocapture
```

The window is 1213×865 unless `PICTO_GPUI_WINDOW=WxH` says otherwise; the
app settles for 3 s before the first line. Set `PICTO_GPUI_MENU=window` on
macOS, where menus would otherwise be pop-up windows of their own, which a
shot of the main window does not show. Without `PICTO_GPUI_STATE=<folder>` the tool keeps no state, so
pictures never depend on how the app was left; two runs with the same folder
show a restart. Script lines (blank lines and `#` comments are skipped):

| Line | Effect |
| --- | --- |
| `move X Y`, `drag X Y` | pointer moved, `drag` with the left button held (window points) |
| `down X Y [clicks]`, `up X Y [clicks]` | left button, with a click count |
| `rdown X Y`, `rup X Y` | right button |
| `scroll X Y DISTANCE` | wheel by DISTANCE points (negative scrolls down) |
| `key <keystroke>` | a keystroke as GPUI writes them (`space`, `cmd-shift-j`), held modifiers reported around it |
| `type <text>` | text typed into the focused field |
| `dragfiles X Y PATH…`, `drop X Y PATH…`, `dragexit` | files from the system over the window, dropped there (imports into the copy), or taken away; paths cannot hold spaces |
| `wait <ms>` | let the app run |
| `pace <ms>` | how long the app runs after each input line (default 200) |
| `shot <name>` | save `<name>.png` |
| `window <n>` | send what follows to the nth window (0 is the main one, others in the order they opened) |
| `lazy`, `eager` | draw only when a real window would / redraw every tick (default) |
| `notify <tone>[+] <title> \| <message>` | show a notification (`error`, `warning`, `info`, `success`; `+` adds a Redo button) |
| `action <name> [n]` | choose an application menu row (`action app_menu::SetRating 3`) |
| `invoke <command> <json>` | send a core command and print its answer |
| `focused` | print the control with the keyboard and whether Tab put it there |
| `answer <path>`, `answer` | answer the next system file, folder or save dialog with that path, or cancel it; give it before the line that opens the dialog |
| `desktop` | append the desktop requests since the last one, and the clipboard's text, to `desktop.log` in the output folder |
| `reload`, `size W H`, `js …` | for the reference only; skipped here |

Nothing in the tool reaches the desktop: opening, revealing, Open With and
clipboard writes are recorded for `desktop` instead. Video does not play
off-screen (AVFoundation needs the main run loop), and the tool cannot resize
a window.

Eager mode redraws every tick, which hides a view that changes state during a
draw without asking for a frame. `lazy` (or `PICTO_GPUI_LAZY=1`) draws only
after something invalidated the window outside a draw or asked for an
animation frame, as a real window does; `PICTO_GPUI_LAZY_DEBUG=1` prints a
line for each lazy frame. Run a script both ways and treat any difference as
a fault.

### Driving a real window

A debug build plays input lines into its main window from
`PICTO_GPUI_POINTER=<path>` (`src/pointer.rs`), so hover, pressed, tooltip,
menu, rubber-band and keyboard states can be checked without taking the real
pointer. The path must be a FIFO: the reader reopens it each time it ends, so
a plain file is played again from the start every 20 ms, forever. It takes
the input lines of the table above (`move` to `dragexit`); clicks carry no
modifiers, so use `key` for modifier shortcuts.

```sh
mkfifo /tmp/picto-pointer
PICTO_GPUI_POINTER=/tmp/picto-pointer PICTO_GPUI_BACKGROUND=1 PICTO_GPUI_WINDOW=1213x865 \
  target/debug/picto-gpui-spike /tmp/picto-gpui-library.XXXXXX/Name.library &
echo "move 320 24" > /tmp/picto-pointer
```

GPUI stops painting a window that is fully covered; `PICTO_GPUI_BACKGROUND=1`
opens the window without taking the keyboard and keeps it drawing behind
other applications (`src/covered.rs`). `PICTO_GPUI_WINDOW=WxH` opens it
centred at that size and does not remember it. A pop-up menu closes when the
app is not the active one; `PICTO_GPUI_MENU=window` keeps menus in the
window.

### The production reference

`reference.sh /path/to/Name.library [n]` starts the Electron app from this
checkout as an isolated reference: a development build in its automation
mode (`PICTO_E2E=1`), on its own copy made by `run.sh` and detached by the
GPUI binary (`--detach`), with its own application data (`electron-data/`
beside the copy) and a DevTools port 9222 + n. It needs `node_modules` at
the repository root and in `native/picto-node`, the native addon built
(`node scripts/build-native-dev.mjs`) and the GPUI app built; one Vite server
on port 8181 serves every instance.

`node reference.mjs steps.txt outdir` plays the same script lines into it
over the DevTools protocol (port from `PICTO_REFERENCE_PORT`, default 9222),
plus `size W H` (viewport in points, pictures at twice that), `reload` (start
from a freshly loaded window) and `js <expression>` (print its value). Start
a script with `reload` and `size 1213 865`; the GPUI tool skips those lines.
The reference does not take `answer`, `notify`, `action`, `focused`,
`desktop`, `lazy` or `eager`, and its system dialogs cannot be answered from
a script. Its CSS animations do not run while its window is hidden; a script
that photographs a dialog first sets their durations to 0 with `js`. Both
apps open the inspector at its default width; the reference has no traffic
lights and shows the default library avatar.

`pixel_diff.py reference.png candidate.png outdir [--region NAME=X,Y,W,H]…`
compares two captures with the Python standard library only and reports the
share of pixels differing by more than `--threshold` (default 16) for the
whole image and each region, with a side-by-side image and a mask.
`--shift DX,DY` moves the candidate first, to tell a uniform offset from real
differences; `--crop` compares the shared top-left area of captures a few
pixels apart. Capture both apps at the same size with the same content,
scroll, selection and focus; never stretch a capture.

### Environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `PICTO_GPUI_FPS`, `PICTO_GPUI_STATS` | every build | frame meter, counters (section 2) |
| `PICTO_FFMPEG_DIR` | every build | where `ffmpeg` is |
| `PICTO_GPUI_POINTER` | debug builds | input lines from a FIFO |
| `PICTO_GPUI_BACKGROUND` | debug builds | open without the keyboard, keep drawing while covered |
| `PICTO_GPUI_WINDOW=WxH` | debug builds, off-screen tool | exact window size, not remembered |
| `PICTO_GPUI_MENU=window` | debug builds | menus in the window on macOS |
| `PICTO_GPUI_APP_MENU=window` | debug builds | the in-window application menu button on macOS |
| `PICTO_GPUI_STATE=<folder>` | debug builds, off-screen tool | keep per-machine state there |
| `PICTO_GPUI_INSPECTOR=<width>` | debug builds | inspector width whatever was kept |
| `PICTO_GPUI_HOLD_RUNS` | debug builds | the Subscriptions screen logs its commands and answers runs and gallery downloads itself instead of starting network work |
| `PICTO_GPUI_SILENT` | debug builds | every player and Flash movie starts muted |
| `PICTO_GPUI_COPY`, `PICTO_GPUI_SCRIPT`, `PICTO_GPUI_OUT` | off-screen tool | the copy, the script, the output folder |
| `PICTO_GPUI_LAZY`, `PICTO_GPUI_LAZY_DEBUG` | off-screen tool | start lazy; log lazy frames |
| `PICTO_GPUI_TEST_LIBRARY`, `PICTO_GPUI_TEST_WEBM`, `PICTO_GPUI_TEST_AUDIO`, `PICTO_GPUI_TEST_VIDEO` | ignored tests | inputs |
| `PICTO_GPUI_MATRIX_OUT`, `PICTO_GPUI_MEDIA`, `PICTO_GPUI_MEDIA_MIME` | format matrix tests | pictures folder, a real file, its type |
| `PICTO_GPUI_COPY_ONLY` | `run.sh` | make the copy, print it, stop |
| `PICTO_GPUI_MILLION_ONLY` | `million.sh` | make the library, print it, stop |
| `PICTO_REFERENCE_PORT` | `reference.mjs` | DevTools port |

### CI

`.github/workflows/gpui-experiment.yml` runs on pushes to `feature/163-**`
that touch `experiments/gpui/`, `core/`, `picto-library/` or the workflow,
and by hand. On Ubuntu 24.04 and Windows (there is no macOS job; macOS is
built locally) it runs `cargo build --locked` and `cargo test --locked`, then
generates a 2,000-item synthetic library in the system's temporary folder,
copies it with `--make-copy`, and plays `ci/smoke.txt` with the off-screen
tool: the grid, a hovered tile, the grid scrolled, the viewer, an item's
menu, the window's close button hovered, the application menu (both drawn by
the app on these platforms) and the Settings window. The eight pictures are
uploaded as the `pictures-linux` and `pictures-windows` artifacts
(`gh run download <run> -n pictures-linux`). They show the app's own drawing
on each system, not the system's: no window frame, no system menus, no
pointer.

## What works

How parity was established, per area: **pictures** — the same script
photographed in this app and in the production reference and compared;
**values** — numbers ported and asserted against production's own tests or
hand-computed geometry; **tests** — unit tests of the port only; **read** —
ported by reading production's code, seen in this app but not compared. The
pictures were compared when each area was built and are not kept in the
repository. The whole main window at 1213×865, same item selected, differed
from production in 4.4% of pixels by more than 16 levels (titlebar buttons
0.2%, toolbar 0.2%, library header 1.1%, system rows 1.4%, section headers
2.2%, sidebar filter 1.6%, grid 5.2%, mostly originals production loads for
large tiles; inspector fields 0.9%, chips 4.3%).

| Area | At parity with production | Established by |
| --- | --- | --- |
| Shell | titlebar, rails that slide and resize (sidebar 220–500 px, inspector 260–550 px), window rim, Back and Forward over places, destination fades, themes (all seven, previewed in every window) | pictures; values (rail limits) |
| Sidebar | system rows in production's order, Quick Access, Folders and Smart Folders with counts, tree guides, folding, filter, multi-selection, rename in place, drag reorder and nesting, items dropped from the grid, every context menu row, smart-folder editor, auto tags, watched folders, keyboard | pictures (sidebar, rules dialog); values (drop positions, rule model) |
| Grid | three layouts placing tiles where production does, tiles and badges, windowed model over the whole destination, selection by item (click, platform key toggle, shift ranges, Select All, rubber band with auto-scroll), keyboard movement, hover preview, thumbnail reveal, originals for large tiles, subfolder tiles, empty and error states, drag out, drop import | pictures; values (layout, visibility, marquee, menus) |
| Filter bar and view menu | every filter control and editor, saved filters, lock, view options saved to the destination's view | pictures (bar, tag picker in filter mode, colour editor, view menu); read (calendar, range editors, saved filters) |
| Inspector | one item, several items, the destination; name, notes, sources, folders and tags, rating, palette, properties; tag and folder pickers; Add Tags and Folders dialogs; pin | pictures |
| Item menus | tile, empty space, viewer and inspector preview menus with production's rows, order, labels and enabled rules; copy, open with, batch rename, regenerate thumbnails, grouping, export, Set as Library Cover, Set as Thumbnail | pictures (grid menu); values (production's menu test cases) |
| Application menu | production's menus and rows, keys from the registry, enabled by production's rules; in-window button off macOS | values (model tests); pictures of the in-window menu off-screen (CI) |
| Viewer | media view, zoom and pan, navigator, Quick Look, detail windows, Inbox review, collections (reader and editor), Recently Viewed recording | pictures (Quick Look, detail window, JPEG, GIF, JPEG XL, a PDF collection); values (camera arithmetic, detail window size) |
| Documents | PDF, DjVu, CBZ, EPUB, text, Markdown, JSON, RTF, fonts, JPEG XL, SVG; others show the unsupported view | tests (format matrix decodes every fixture and draws every viewer off-screen; real PDFs, a JPEG XL, a text file, a GIF and a WebP from a library) |
| Video and audio | the media player, its controls, speeds, loop, volume, visualizer; AVFoundation and `ffmpeg` paths on macOS | pictures (`.mov`, `.webm`, MP3); seen playing in a real window on macOS |
| Flash | Ruffle playback, controls, Set as Thumbnail | tests (GPU tests: frames, mouse, Stop) |
| Tags screen | rail, groups, sections, paging, search, every edit | pictures |
| Subscriptions screen | grid of cards, workspace, Sources, History and Problems tables, every dialog and command | pictures; commands checked with `PICTO_GPUI_HOLD_RUNS` |
| Duplicates screen | queue, linked zoom, difference view, every decision, scan | pictures; values |
| Imports and exports | drops, choosers, paste import, folder and collection questions, task dialog and bar; Export Originals, Export As, Picto Packs | pictures (drop, folder dialog, collection question, task dialog and bar); read (exports, with dialogs answered off-screen) |
| Settings | every section and row in production's wording; the settings that change the app apply on Save; shortcut capture and conflicts | pictures |
| Keyboard | every registry shortcut bound or listed in `NOT_YET`; Tab order and focus rings in dialogs, popovers and Settings | values (keymap test); read |
| Text fields | Chromium's editing on each platform: words (with Chinese, Japanese and Thai dictionaries), rows, paragraphs, Emacs keys, the field's own undo, input method | tests |
| Glass and blur | backdrop blur behind panels on Metal and wgpu, shadows cut out as CSS draws them, menus in pop-up windows over the system's blur on macOS | read; seen on macOS |
| Notifications and undo | production's notifications, tones and words; Undo and Redo announced | read |
| Kept across launches | window place, rails, folds, filters, recents, shortcuts | tests (pure parts) |

Shortcuts `keymap::NOT_YET` leaves unbound: command palette, go to folder,
add to last folder, copy and paste tags, slideshow, navigator, the layout and
tile-label keys (all deferred in production too), Auto Tag (no AI tagger) and
Toggle Logs (no log window).

Measured once on the million-item synthetic library on a macOS development
machine: every grid window read took 0.3–9 ms wherever it lay; the summary of
a million selected items 0.6 ms; opening the database and its resident
indexes 1.2–11 s (varying with the file cache); the core's recovery passes
13 s alone and 28–30 s with the app drawing beside them. A copy of a 24 GB
library through `run.sh` took about 8 s.

## What does not work

Entries marked † were observed when the area was built and could not be
re-checked by reading the code; the rest were checked against it.

### Never seen in a real window or on a real machine

- **Anything on Windows or Linux.** No one has used the app on a screen
  there. CI builds, tests and photographs its own drawing; the systems' window
  frames, titlebar dragging, resize edges, double-click on the titlebar, the
  clipboard, notifications, the badge, keyboard input, video and sound have
  never been exercised. The `ffmpeg` video path and the Symphonia and CPAL
  audio path are built and unit-tested there but have never played a file
  (CI has no `ffmpeg` and no sound device).
- **macOS menu bar.** The real menu bar has not been looked at. AppKit adds
  rows of its own (Edit's Dictation and Emoji & Symbols, Enter Full Screen,
  the Window menu's tiling rows) and titles the application menu with the
  process's name.
- **macOS system integration.** Dragging items out to Finder; the Dock
  badge and the system notification (which need the app to run from its
  bundle); a real pasteboard copy of files; files arriving through
  `application:openURLs:` (the hand-over between two launches and packs on
  the command line were seen working); launching an application through
  Open With; the media player's full-screen transition; restoring the window
  maximised or on a second display (only the pure placement code is tested).
- **Pop-up menus on macOS.** They have been seen reaching past the window,
  with a submenu, and closing on a press in the main window. Not seen: their
  blur over colourful content, on a submenu or past the window's edge; the
  pointing-hand cursor in them (GPUI sets the cursor from the key window);
  the input method's candidate window for the menu's search field. A select's
  list inside a pop-up menu is checked by placement tests only.
- **Network work.** No subscription run, source run or gallery download has
  been started from this app; their commands were checked with
  `PICTO_GPUI_HOLD_RUNS`.
- **System dialogs with the pointer.** Paste Import, the file and folder
  choosers, the export and Picto Pack dialogs and the empty states' import
  buttons have been driven only off-screen with `answer`.
- **Not even off-screen:** the filter bar's calendar, range editors and saved
  filters; the document viewers, font viewer and Flash controls with a
  pointer (hover, tooltips, scrolling, the volume drag, the controls' fade);
  the media player's timings, hover states and volume and rate pop-ups;
  Set as Thumbnail end to end; the rubber band in the collection editor.
- **Performance.** No frame-time, decode-latency or memory benchmark has
  been completed. Do not call the app faster, flicker-free or
  memory-bounded on this evidence. Per-frame work is bounded by the grid's
  window by construction and test, not by measurement at scale.

### Missing features

- Opening, creating or switching libraries: the library switcher's rows,
  Create Library, Open Library and Library Manager only close the popover;
  the application menu's Library Manager, New Library, Open Library, Recent
  Libraries and Clear History are disabled.
- Localization: English only; the Language setting is saved and read by
  nothing (production has eight locales).
- App-wide UI zoom (production's ⌘= / ⌘− / ⌘0 outside the viewer): every
  size is in pixels. Settings' Zoom Level is saved and, as in production,
  not applied.
- Accessibility: the native window is exposed, the control tree is not (the
  vendored GPUI includes AccessKit; the app sets no roles or labels).
- The AI tagger: Auto Tag in the item menu, the inspector and the Organize
  menu is disabled, its shortcut unbound; every AI model shows as not
  downloaded and download, optimise and delete do nothing.
- The log window, diagnostics and support report; no panic hook.
- Update checks, cloud sync, pause, snapshots, restore points and retention
  writes: drawn in Settings, not performed.
- Site logins and the Accounts window; a login problem's Fix login; the
  Problems tab's other recovery actions (Retry now, Review source) and its
  detail tooltip; a subscription run's own pop-ups ("Subscription paused",
  "Query completed", "Subscription completed").
- Search by Image (disabled in every menu).
- Drops from a browser (GPUI delivers only file paths, so in-memory files
  and links do not arrive).
- Opening another library from the file manager or the command line of a
  running instance (production's `switchLibrary`): it is refused with a
  notification.
- On macOS, a pack opened with the app while no instance runs: the system
  launches the bundle without a copy, which the app refuses.
- Thumbnails for text, Markdown, JSON, RTF, EPUB, Word and PowerPoint files
  (production draws them in a hidden window); they show the torn page.
- Document viewers: PDF text selection, text selection on text and book
  pages, the editable font sample, WOFF2 fonts, RTF character formatting,
  Markdown images and raw HTML (shown as text)†, an EPUB chapter's own
  stylesheet†.
- Detail windows: per-image zoom memory, "Loading..." placeholder†, a menu on
  a collection's members, and keeping the toolbar shown while the window is
  dragged.
- "Ungroup N Collections..." names no count when the whole destination is
  selected and not all loaded.
- Selecting the new group after Group..., the organizer or a merge.
- Reduced motion: the system setting is not read.
- File choosers take every file type (GPUI's prompt has no type filter).

### Waiting on the owner

- Whether to open, create and switch real libraries, and how (Library
  Manager).
- Site logins: a web view or the system browser.
- Network features: update check and install, cloud sync, AI model download.
- Two changes to the core made for this app: "Add open_library_reporting,
  which hands over the library before recovery runs" (the app depends on it)
  and "Make the open's media recovery pass list thumbnails once instead of
  looking for each": keep or drop.
- File › New Folder starts a rename here; production's does not.
- The "Subscriptions finished" system notification follows Settings' pop-up
  switch and the Information tone here; production always sends it.
- The sidebar's row keyboard (Down or Tab from the tree filter enters the
  rows) has no production counterpart (there Down does nothing and Tab goes
  to the grid): keep it or not.
- A verdict on the context menu's pop-in and the pop-up menus' blur.

### Kept different on purpose

- PDF, DjVu and comic pages scroll as one continuous column, because the
  owner asked; production shows one page at a time. In a collection the
  pages run inline at the member's width.
- Word and PowerPoint files are not viewed: they keep their embedded
  thumbnail and open in the default application.
- Organize › Tags opens the Tags screen; production's row does nothing.
- Copy in the application menu, outside a text field, copies the selection's
  files as ⌘C does; production's row, chosen with the pointer, copies nothing.
- Failures production leaves silent are reported: a failed Copy, Regenerate
  Thumbnail or Set as Thumbnail, and every export's result and first skipped
  file.
- Set as Library Cover writes `.picto-library-cover.*` beside the copy's
  database, not production's global configuration.
- Keyboard moves, Home, End and the page keys go across the whole
  destination; production's stop at the ends of the loaded window.
- On macOS, Close Window hides the main window and the Dock icon brings it
  back with its state; production makes a fresh one. Off macOS, closing the
  main window quits, and detail and Settings windows go with it; production
  can leave them open on their own.

### Production bugs not copied

- Production's window-wide shortcut handler swallows Tab on every control
  that is not a text field (toggling both panels, behind a dialog too), sends
  Enter, Space and the arrows to the grid or viewer behind a dialog, and
  traps no focus in dialogs. Here Tab cycles inside a dialog, popover or
  Settings and keys act on the focused control; outside them Tab still
  toggles the panels.
- Production's folder drag ghost is drawn 48 px below the pointer; here it
  sits on the pointer as `Sidebar.tsx` intends.
- In a folder showing its subfolders' items production shows a reorder
  marker and then fails; here there is no marker.
- With one subfolder tile selected the inspector shows that folder;
  production keeps the parent's scope (or shows no covers or size).
- A cover saved as a rendered crop shows here; production's media protocol
  falls back to the download glyph for it.
- One dropped path that is not a directory is never sent to the folder
  import dialog; production sends any one path without a media extension
  there (a single Flash, EPUB or audio file), where it cannot be imported.

### Known differences not yet fixed

Text and keyboard
- A line starting with right-to-left text is laid out right to left with no
  selection drawn†; no spell-check underline; Home and End in the
  inspector's fields move the caret where production's only scroll†.
- A radio group is one tab stop but its arrows do not move the choice; a
  select has no typeahead.
- `+` also answers to Shift+= (GPUI matches the typed character)†; Space
  pages the grid at once rather than smoothly†.
- A review key held down is told from its repeats by its release or the
  next key (GPUI hands an action no repeat flag)†.

Navigation and results
- A filter change or a search is not a step of Back and Forward, and the
  grid returns to its top.
- A collection reorder or sort that fails is written only to standard
  error; a detail window whose item fails to load stays blank.
- The Tags screen shows changes made elsewhere only when it is opened again,
  and Undo does not reach tag edits†.
- Settings reads the library's statistics and cloud state once when it
  opens; the keyboard preset, audio visualization and shortcut edits apply
  only on Save, without production's live preview†; an undo that reverts a
  settings write does not reload the model†. "Reset all views" deletes the
  `view_pref` rows with its own SQL rather than through the core's
  `settings.view.reset`.

Drawing (GPUI cannot scale or rotate an element's contents, and has no
radial gradients, masks or blend modes)
- Where production scales a panel (menus, dialogs, tooltips, the library
  popover, the task dialog, the volume display), only the surface, border and
  shadow scale; the contents stay at full size.
- Radial gradients drawn as linear (folder card, subfolder tile cover, audio
  tile) and the audio tile's `brightness(1.5)` not applied†; the turned stack
  card in the inspector is a bitmap made off the drawing thread and lies
  straight until it is ready.
- Rounded corners do not clip a zoomed image (Quick Look, Duplicates); the
  difference picture is computed from decoded images, at most 4,096 px on its
  longest side.
- A fading rail or dialog still takes the pointer (production sets
  `pointer-events: none`)†.
- The glyph grid's hovered glyph is not enlarged.

Smaller differences by area†
- Inspector: a selection's covers follow the order items were chosen in, not
  the query order, for shift ranges; Auto Tag is drawn but disabled.
- Pickers and dialogs: with several items selected a picker starts from the
  tags or folders they share; the dialogs' fold chevron turns at once and row
  highlights do not fade; a folder with its own glyph shows the plain folder.
- Sidebar: the activity button's tooltip opens below it, not to its right;
  section headers are not in the keyboard order; the smart-folder notes field
  does not scroll past three lines; its file types are the six built-in
  choices; the icon picker shows the sidebar's filled glyphs.
- Filter bar: the colour editor's hue strip is six bands, its square is not
  clipped round, and the hex field neither capitalises nor stops at seven
  characters; menus that hold editors stay inside the window.
- Library popover: only its surface scales in; a library's folder has no
  hover title; a cover changed elsewhere shows the old one until restart; a
  library with an icon and no cover shows the books glyph.
- Duplicates: the progress bar over a shown pair is a 2 px row above the
  cards; the Smart merge ring follows the pointer, not keyboard focus; no
  per-card "Loading metadata…".
- Quick Look: its counter follows the selection at once; stepping scrolls the
  grid only enough to show the item; closing does not scroll back.
- Detail window: the pinned button rests at full strength (production 94%);
  its size uses the primary display's work area, as production's does.
- Subscriptions: the cover's crop is drawn from the thumbnail; truncated
  cells have no title tooltip; the History tab does not fall back to "N
  invalid metadata".
- Imports: the task dialog does not grow in from 96% and its focus rings are
  not drawn.
- Media player: the rate menu is production's own small list, not the shared
  menu; the visualizer's reflection and glow are approximations; an APNG
  plays as a still image; Flash's stage is 960×720 rather than fitted to the
  movie, and the media keys do not reach Flash.
- Documents: a page larger than the view starts at its top-left; only the
  vertical scrollbar is drawn; text files over 4 MB show their first 4 MB and
  say so; a PDF page is drawn at most 8,192 px on its longest side;
  encrypted PDFs show a message; Quick Look scrolls a document's pages with
  the pointer only.
- Settings: a stored vibrancy, Liquid Glass, Mica or Acrylic theme shows as
  Dark; a shortcut field cannot capture keys the app binds itself (⌘Q, ⌘,);
  "Pixelated" scaling does not apply to a collection's members or the
  navigator, as in production.

### Per platform

| | Windows | Linux X11 | Linux Wayland |
| --- | --- | --- | --- |
| Backdrop blur | none (the DirectX renderer ignores it); a more opaque glass instead | yes (wgpu) | yes (wgpu) |
| Context menus | in the window | in the window (an X11 pop-up takes no keyboard) | in the window (GPUI cannot place a pop-up there) |
| Application menu | titlebar button | titlebar button | titlebar button |
| Window buttons and drag | the app's buttons as system caption areas; the titlebar is the system's caption | the app's buttons, client-side resize edges | same as X11 |
| Inbox badge | none (as production) | Unity launcher D-Bus signal | same |
| System notification | toast through GPUI under the app's identity | freedesktop notification | same |
| Copy files | `CF_HDROP` | `text/uri-list` through `xclip`, which must be installed | through `wl-copy`, which must be installed |
| Open With Other | the system's chooser (`rundll32 shell32.dll,OpenAs_RunDLL`) | none (as production) | none |
| Drag items out of the window | not built (needs OLE `DoDragDrop` outside GPUI's event dispatch) | not built (needs an XDND source) | GPUI's native drag |
| Single instance per copy | named pipe; the instance may take the foreground (`AllowSetForegroundWindow`) | Unix socket in `$XDG_RUNTIME_DIR` (else the temporary folder) | same as X11 |
| Packs opened from the file manager | command-line arguments | command-line arguments | same as X11 |
| Detail window aspect lock | not built | not built | not built |
| Detail window always on top | yes | no; the pin is hidden | no; the pin is hidden |
| UI font | the system's (production bundles Geist) | the system's | the system's |
| Video | bundled `ffmpeg`, frames uploaded per frame | same | same |

On every platform the Ruffle frame path reads back from the GPU and uploads
again, and only AVFoundation video is drawn without a copy.

## Attribution

`src/ui/text_input/` adapts GPUI 0.2.2's official `examples/input.rs`,
Apache-2.0; see `LICENSE-gpui`. The vendored GPUI crates are Apache-2.0; see
`vendor/README.md`. Sidebar icons are Tabler Icons, MIT; see
`assets/LICENSE-tabler`. Toolbar glyphs are Picto's own, from
`src/shared/ui/icons/toolbar-icons.tsx`. Roboto and Fira Mono are bundled
under their licences (`assets/LICENSE-Roboto.txt`,
`assets/LICENSE-FiraMono.txt`). `assets/fixtures/interactive.swf` comes from
Ruffle's `tests/tests/swfs/avm1/button_goto/test.swf` at the Ruffle revision
pinned in `Cargo.toml`; see `assets/LICENSE-Ruffle.md`. The files in
`assets/fixtures/documents/` were made for this project (drawn pages, encoded
with zip, djvu-rs and zune-jpegxl, a hand-written PDF and text).
