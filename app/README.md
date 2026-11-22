# Picto, the app

## 1. What this is

This folder holds the app that is released as Picto. It is written in Rust
on GPUI, the user interface framework of the Zed editor. It runs Picto's
application core and library (`core/`, `picto-library/`) in its own process:
it sends the core's commands, and the core's background workers and events
run inside it.

The app covers what a user does day to day: browsing, selecting, the
inspector, the menus, the viewers, imports, exports, the Tags, Subscriptions
and Duplicates screens, and Settings. Section 7 says what works, what was
checked on each system, and what is open.

Picto 0.6.19 and earlier were built with Electron. This app is their next
version: a release build has the same name and the same id, and its packages
have the same file names and install over those versions (section 3). Where
this document compares the app with those versions, it calls them "Picto
0.6.19".

A debug build opens a disposable copy of a library, and it opens a library
the user keeps only under the rules of section 2. The app is developed and
used on macOS. On Windows and Linux it is built, tested and photographed
off-screen in CI, and it has never been used on a screen there.

The crate and the program of a debug build are named `picto-gpui-spike`. The
packaged program is named `Picto`.

Issue [#163](https://github.com/midona-rhel/picto/issues/163) lists what is
open.

## 2. Running it safely

### The safety boundary

The app writes, imports and deletes files, so a folder opens as a throwaway
copy only when `application::guard` (`src/application/copy.rs`) proves it is
one:

- it holds the marker file `.gpui-disposable`, whose first line names the
  library it was copied from (or `synthetic:…` for a generated library);
- it is not that library, not inside it, and does not contain it;
- it sits exactly one level inside a folder named `picto-gpui-…` directly in
  a temporary folder: the system's (`std::env::temp_dir()`: `$TMPDIR` on
  macOS, `%TEMP%` on Windows) or, on macOS and Linux, `/tmp` or `/var/tmp`.
  A marked folder anywhere else is refused, since a marker travels with the
  files;
- none of its top-level entries, none of the entries of `blobs/`, and
  neither `blobs/f` nor `blobs/t` is a symbolic link;
- its `library.sqlite` is not the source's database reached another way (a
  hard link).

Before the workers start, on every launch, `application::detach` cuts the
copy loose from the real library's work, in one transaction: cloud sync
disconnected; a cloud identity of the copy's own given once (a new
`library_id` and `device_id`, in `.gpui-cloud-identity`), so a copy connected
to a cloud folder writes and purges only under its own name
(`a_copy_never_syncs_as_the_library_it_was_copied_from`); every watched
folder switched off; every subscription's next run cleared and pending or
running runs and run queries cancelled; "delete after import" cleared on
every import job. On the first open only (it then writes `.gpui-detached`)
it also fails import jobs still pending or running, and rewrites every
`media_file.file_path` that starts with the source's path to start with the
copy's, so nothing opened, revealed, copied or dragged out is a file of the
real library.

The core's application data outside a library (its AI models, in `models/`)
is, in a debug build, `app-data/` beside the copy or the folder
`PICTO_GPUI_APP_DATA` names, never the folder that an installed Picto uses
(`application::application_data`). The core may write there whenever it
opens, so to try the AI tagger with models that an installed Picto
downloaded, point the variable at an APFS clone of them (`cp -Rc "$HOME/Library/Application
Support/picto/models" <folder>/models`), never at the real folder. A release
build uses `picto` in the per-user application data folder. Picto 0.6.19 and
earlier use the same folder, so a release build finds the models that such a
version downloaded.

Files reach the rest of the desktop only through `src/desktop/` (open with
the default application, Open With, reveal, copy files, Copy File Path, drag
out). Each makes the path canonical and refuses, with a notification, any
file not inside the open library, whatever the library stored; the paths are
handed over without Windows' `\\?\` prefix (`desktop::system_path`). Links
open only when they are `http:` or `https:` (`desktop::open_link`). Delete
Permanently, Empty Trash and the Duplicates screen's merges remove files
only through the core, from the open library's blobs. The
source of a copy is only ever read.

### Opening a library you keep

A library the user keeps (a `.library` folder with `library.sqlite` and no
marker) is that user's only copy of years of work. `src/libraries/admission.rs`
decides every open, in this order; each rule names where it is enforced and
the test that proves it.

1. **It is a library, and not a copy out of place.** A folder that passes
   the guard opens as a copy. A marked folder that fails it is refused with
   the guard's words. Anything else must be a `.library` folder holding
   `library.sqlite`; an empty `.library` folder is refused
   (`a_copy_opens_as_before_and_a_marked_folder_elsewhere_is_refused`,
   `what_is_not_a_library_is_refused`).
2. **The build permits it.** A release build does; a debug or test build
   only when started with `--allow-real-libraries` as its first argument
   (`main.rs`, `libraries::set_flag`), so every agent, script, test and CI
   run is copy-only (`a_debug_build_refuses_a_kept_library_without_the_flag`).
3. **No automation seam is active.** Any variable of `src/seams.rs`, or a
   test build, refuses a library the user keeps even with the flag
   (`seams::automation`;
   `an_automation_seam_refuses_a_kept_library_even_with_the_flag`). A
   release build has no seams.
4. **The user asked for it**: the Library Manager, Open Library…, Recent
   Libraries, the sidebar's switcher, the library last open (`lastLibrary`
   in this app's `state.json`), a library found with Locate…, or the command
   line with the flag. A missing last library asks the "Library
   Not Found" question (Locate…, Remove from List, Cancel;
   `host::start_last`); a library Locate… finds takes the old one's place
   and is opened through every rule here
   (`locate_finds_the_library_chosen_or_the_one_inside_the_folder`). A file
   the system hands over never opens a library (`opening::accept`,
   `only_a_library_the_user_chose_is_opened`).
5. **No other Picto is running.** Picto 0.6.19 and earlier take no lock
   that this app can see, so the app looks at the running processes
   (`src/libraries/platform/`: the executable paths on macOS, `/proc` on
   Linux, a Tool Help snapshot on Windows). A process is a Picto when its
   program is named `Picto` or `Picto Helper…` on macOS, `Picto.exe` on
   Windows, and `picto` on Linux, or when it was started from a
   `Picto…AppImage`. Those are the names of a packaged release build of
   this app and of Picto 0.6.19 and earlier. The process that looks is left
out, and on Linux also the
   AppImage runtime that serves its own files. A debug build has other names
   (`picto-gpui-spike`, `PictoGPUI`) and is not counted.

   A release build looks once when it starts, before it does anything else,
   and does not start when it finds a Picto (`exit_if_another_picto_runs` in
   `main.rs`). So only one Picto runs at a time, whichever
   version it is. A debug build does not look at start, because several
   debug builds run at once on purpose, each on its own disposable copy.

   In every build, a library the user keeps is refused while another Picto
   runs, and also when the processes could not be looked at. While such a
   library is open the check runs again whenever the main window becomes
   active, and a running Picto puts a notice over the window until it quits
   (Check Again) or the library is closed (`libraries::host::check_picto`;
   `a_kept_library_is_refused_while_picto_runs_or_cannot_be_checked`). The
   check sees names only. It does not see a Picto that was run from source,
   was renamed, is run by another user, or runs on another machine over a
   network share.
6. **In a debug build, the core finds its schema current**, read through
   SQLite's `immutable` flag, which writes nothing
   (`admission::compatibility`, `picto_library::schema::validate`). A debug
   build refuses another schema with the core's words and never updates it.
   A release build does not look: the core updates a library of an earlier
   schema when it opens it, after making a backup, and the app shows the
   core's message when a library cannot be opened
   (`a_debug_build_refuses_an_incompatible_schema_with_the_cores_words_and_leaves_it_alone`).
7. **In a debug build, the user agreed, once per library.** The first open
   asks, naming the path. The answer is kept in `state.json`
   (`libraryConsents`: canonical path and the database file's identity),
   never in the library; a different library later at the same path is
   asked about again; New Library here needs no question and a rename
   carries the consent. A release build is Picto itself and opens a library
   without the question
   (`a_debug_build_asks_for_consent_once_per_library_and_its_database`).

Then (`libraries::host::begin`) this app's instance lock refuses a second
instance on the library, and the core takes `.picto-library.lock`, an
exclusive lock held while it is open. A library the user keeps is never
detached or marked (`detach_never_touches_a_library_without_the_marker`). A
switch closes the library first (`Application::close`, after which the old
application refuses every command) and closes the detail, Settings and log
windows, whose commands would otherwise reach the next library
(`a_kept_library_opens_as_it_is_and_a_switch_closes_it_first`).

The first run may list the libraries of an installed Picto 0.6.19 or earlier
(`libraries::seed`): when this app's list is empty and was never seeded, a
process that may list libraries the user keeps reads that version's
`config.json` read-only and copies its `libraryHistory` and
`pinnedLibraries`, once. Seeding gives no consent. A release build also
takes `lastLibrary`, so its first start after an update opens the library
that version had open last; a debug build takes no last library and opens
nothing by itself
(`seeding_happens_once_into_an_empty_list`,
`a_process_that_cannot_list_kept_libraries_never_reads_the_file`). Listing
shows only libraries this process may open (`libraries::listed`) and writes
nothing to them (`listing_a_kept_library_writes_nothing_to_it`).

Edits that cannot be undone work only on the library open in the main
window (`libraries::writable`, through `Session::writable_root`; otherwise
"No library is open.", `edits_are_refused_unless_their_library_is_the_open_one`),
and every file an edit writes itself must lie inside that library's folder
(`libraries::confine`; `writes_are_confined_to_the_open_library`,
`a_thumbnail_is_written_only_inside_the_library`,
`the_cover_is_written_only_inside_the_library`). That covers Delete
Permanently, Empty Trash and Reject when it deletes (the core's own
`Library::permanently_delete` and `delete_rejected`; the core's worker
removes the files), drawn thumbnails, Regenerate Thumbnails, Set as
Thumbnail, and the library's cover.

The Library Manager's Rename, Show in File Manager, Move… and Delete… on a
library that is not open run `admission::admit` first (`host::permit`) and,
in a debug build, ask the consent question. Delete… is
refused for the open library and removes only a real `.library` folder
holding `library.sqlite` that no one else holds locked
(`delete_removes_only_a_library_that_is_neither_open_nor_held`); Move…
across volumes copies, compares every file and only then removes the
original (`a_copy_across_volumes_is_verified_before_the_original_goes`).

### Cloud

Cloud sync signs in to nothing: a library publishes verified
snapshots of its database and originals into the folder of a Google Drive or
Dropbox desktop app on the device, which carries them on. The core owns all
of it (`core/src/cloud/`, commands `cloud.*`); the app sends those commands and
keeps no tokens or secrets. The safety facts:

- A copy's cloud is disconnected at every launch and has its own identity
  (above), so it never writes or purges as the library it came from.
- While any automation seam is active only the folders `PICTO_GOOGLE_DRIVE_ROOT`
  and `PICTO_DROPBOX_ROOT` name are listed or accepted; the core's own
  validation looks under the home folder, so checks run with `HOME` pointed
  at an empty folder.
- **Restore** (`host::restore`) closes the library's views, waits until this
  app holds no connection to the database (at most 15 s, else nothing is
  restored), has the core keep the current database as an emergency copy and
  put the snapshot's in place, and opens the library again.
- **Open from Cloud…** (`host::join`) makes a library the user keeps, so it
  follows the rules above; a failed join removes the folder it made and
  reopens the previous library.

### Making a copy and opening it

On macOS, `run.sh /path/to/Main.library` refuses unless the library and
`$TMPDIR` share an APFS volume, builds the debug binary, makes the copy with
`--make-copy` (APFS clones, taking no space until a file changes), wraps the
binary as `target/Picto GPUI.app` (needed for system notifications) and opens
it. `PICTO_GPUI_COPY_ONLY=1` makes the copy, prints its path and stops.

On every platform:

```sh
cd app
cargo run --locked -- --make-copy /path/to/Main.library   # prints the copy's path
cargo run --locked -- <that path>
```

`--make-copy` (`application::make_copy`) puts the copy in a new
`picto-gpui-library.…` folder of the system's temporary folder, backs up the
database, copies the rest with the system's file copy (clones on APFS,
btrfs, XFS and ReFS, a full copy elsewhere), refuses a library holding a
symbolic link, writes the marker, detaches and guards the copy, and leaves
nothing behind if it fails. On Linux the app finds the application core's
ONNX Runtime library (`libwebgpu_dawn.so`) in its own folder, where the
build puts it (`build.rs` sets the program's run path to `$ORIGIN`), so the
built app can be started directly or through `cargo run`. The test program
is in `target/debug/deps` and has no such run path. To start it directly,
put `target/debug` and `target/debug/deps` in `LD_LIBRARY_PATH` first, as
the CI workflow does.

With no argument the app opens the library it had open last, if it may, and
otherwise shows the gate, which lists the libraries. `--version` as the only
argument prints the app's name, its version and the folder it takes `ffmpeg`
from, and exits before anything else of the app has run. Files after the
copy are opened as files from the file manager are (`src/opening/`): a
`.picto-pack` goes to the pack import dialog; a
`.library` other than the open one is refused.

A debug build has one instance per library: a second launch on the same
library hands its files over and exits, and a launch with files and no copy
hands them to the instance that was active last. A release build is the
only Picto that runs (rule 5 above), so a launch with files and no copy
starts the app as a launch without arguments does and opens the files
itself. The packaged app registers the `.picto-pack` file type with the
system (section 3); a build run from the build folder registers nothing.

`million.sh [items]` generates a synthetic library (a million items by
default, five smart folders) in `/tmp/picto-gpui-synthetic.XXXXXX/` with the
core's `million_grid_demo` example and opens it with the frame meter.

Delete copies when done: `/tmp/picto-gpui-*` and `$TMPDIR/picto-gpui-*`, or
`%TEMP%\picto-gpui-*` on Windows.

## 3. Building

The app is a Cargo workspace of its own (`app/Cargo.toml` and
`Cargo.lock`). The toolchain is Rust 1.98.0, pinned by the repository's
`rust-toolchain.toml`; without rustup, put that toolchain's `bin` first on
`PATH` (`app/.toolchain/` is ignored by Git for that).

```sh
cd app
cargo build --locked
cargo test --locked
```

GPUI is `gpui-pre` and `gpui-pre-platform` `=0.3.12`, a crates.io snapshot of
upstream GPUI pinned exactly (the `gpui` crate has not been published since
0.2.2, and snapshots promise no API stability). Six of its crates are
vendored under `vendor/` and patched in with `[patch.crates-io]`: a backdrop
blur (Metal and wgpu; the DirectX renderer ignores it), CSS-like cut-out
shadows, nearest-neighbour sampling, a native file drag the app starts
itself (`Window::start_platform_drag`, with an OLE source on Windows and an
XDND source on X11, whose file-list encoders the app's Copy uses too), what
a drop carries besides files (`ExternalDropData`, `PromisedFiles`), an
interface zoom (`App::set_ui_zoom`), a way to ask whether the system blurs
what is behind a window (`Window::is_background_blur_supported`), Acrylic on
Windows 11 through the system's documented window attribute, and test-only
hooks for the off-screen tool. `vendor/README.md` explains each change and
holds the diff against 0.3.12.

The `dev` profile builds this crate at `opt-level = 1` and dependencies at
`opt-level = 2`, with line tables only and incremental compilation off, to
keep build output small. Expect about 1,100 packages, a 156 MB debug binary
and about 7 GB of `target/`.

The `release` profile (`cargo build --locked --release`) is the one the
packages are built with. It optimizes fully, uses thin link-time
optimization, strips the symbols and keeps debug assertions off; the
comments in `Cargo.toml` say what each setting is for. Without debug
assertions the program holds none of the test seams of `src/seams.rs`, keeps
logins in the system keychain, uses the application data folder `picto`
(section 2), has the name Picto and the id `com.picto.desktop`
(`APP_NAME` and `APP_ID` in `src/desktop/mod.rs`), and does not start while
another Picto runs. On Windows a release build has no console window.

Linux needs: `pkg-config clang cmake libasound2-dev libclang-dev
libdbus-1-dev libfontconfig-dev libglib2.0-dev libssl-dev libvulkan1
libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libzstd-dev libudev-dev
libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
libjavascriptcoregtk-4.1-dev` (and `mesa-vulkan-drivers` for software
drawing; WebKitGTK 2.38 or later at run time for the sign-in window). On
Windows the sign-in window needs the WebView2 Runtime, and a build needs the
Windows SDK, whose resource compiler puts the icon and the version
information into the program (`build.rs`, through the `embed-resource`
crate that GPUI's build already uses).

Video beyond what AVFoundation plays needs the application core's `ffmpeg`
and `ffprobe` programs. The core and the source providers look for them in
the folder `PICTO_FFMPEG_DIR` names first, then in `vendor/ffmpeg/` in or
above the working directory (`scripts/download-ffmpeg.sh` puts them there),
then on `PATH`. A program that has `ffmpeg` in its own folder, as the
packaged app has, sets `PICTO_FFMPEG_DIR` to that folder when it starts,
unless the variable is already set (`main.rs`).

### Packages

`./package.sh` builds the release program and makes the package for the
system it runs on, in `target/package/`. `ffmpeg` and `ffprobe` must be in
`vendor/ffmpeg/` at the repository's root first (`bash
scripts/download-ffmpeg.sh` there). Arguments of the script go to `cargo
build`. The packages have the file names that every release of Picto has
had, and they upgrade an installed Picto 0.6.19 or earlier.

| System | Files | What is in the package |
| --- | --- | --- |
| macOS, arm64 | `Picto-<version>-arm64.dmg`, `Picto-<version>-arm64-mac.zip`, `latest-mac.yml` | `Picto.app` with the program, `ffmpeg` and `ffprobe` in `Contents/MacOS`, `packaging/macos/Info.plist`, and the icons and the folder `licenses` in `Contents/Resources`. The disk image also holds a link to the Applications folder. |
| Windows, x64 | `Picto-Setup-<version>.exe`, `latest.yml` | An NSIS installer (`packaging/windows/installer.nsi`) with `Picto.exe`, the DLLs the build put beside it (`DirectML.dll`, which the application core's ONNX Runtime uses), the Microsoft C++ runtime DLLs that these import (such as `vcruntime140.dll`), `ffmpeg.exe`, `ffprobe.exe`, the icon of pack files and the folder `licenses`. Needs NSIS 3 (`makensis`), Git Bash and Visual Studio or its Build Tools with the C++ tools for x64. |
| Linux, x64 | `Picto-<version>.AppImage`, `latest-linux.yml` | An AppImage with `usr/bin/picto`, `libwebgpu_dawn.so`, `ffmpeg` and `ffprobe` beside it, `packaging/linux/AppRun`, the desktop file, the icon and `usr/share/licenses/picto`. Needs `appimagetool`, on the `PATH` or named by `APPIMAGETOOL`. |

The version is the one in the repository's `package.json`. The `.yml` files
are update feeds in the format electron-builder writes: the version, each
package with its SHA-512 and its size, and a release date. The app's updater
(`src/updates/feed.rs`) and the updater of Picto 0.6.19 and earlier read
them. The files
`*.blockmap` that electron-builder also writes are not made; an updater that
does not find one downloads the whole package.

On macOS the app's icon is compiled from `build/Picto.icon` with `actool`
when Xcode 26 or later is installed. The picture of the icon's layer is drawn first by
`scripts/generate-app-icons.mjs`, which needs Node.js and the repository's
packages (`npm ci`). Without Xcode 26 the icon is made from
`build/icon-flat.png`. The app is signed
with the project's certificate "Picto Code Signing" when `MAC_CSC_LINK` and
`MAC_CSC_KEY_PASSWORD` are set (`scripts/sign-mac-app.mjs` and
`scripts/mac-signing.mjs`), and the script says so plainly when it is not.
`Info.plist` declares `.picto-pack` as a file the
app opens, and `.library` as a folder that the Finder shows as one file; it
does not offer the app as the program that opens a `.library`, because the
app does not open a library the system hands to it (section 2, rule 4).

The Windows installer uses the registry keys, the folder, the shortcuts and
the arguments of the installer that electron-builder made for Picto 0.6.19
and earlier, so it upgrades such an installation in place: it runs the old version's own
uninstaller, which removes the old files and keeps the user's data, and
then installs into the same folder. It accepts `/S`, `--updated`,
`--force-run`, `/currentuser`, `/allusers` and `/D=<folder>`. A folder for a
new installation that is not named `Picto` gets a `Picto` folder inside it.
It registers `.picto-pack`, and its uninstaller deletes the installed files
one by one, by name, from a list that `package.sh` writes. Neither deletes
application data. It differs from electron-builder's
installer in three ways: its texts are in English only, an upgrade never
asks for the folder or for whom to install, and the uninstaller has no
option to delete application data.

`Picto.exe` loads the Microsoft C++ runtime from DLLs that are not a part of
Windows, so the installer puts them into the program's folder, where
Microsoft allows an application to install them. A computer then needs no
Visual C++ redistributable for the program to start. `package.sh` asks the
linker of the Visual Studio on the build machine which DLLs `Picto.exe` and
the DLLs beside it import (`link.exe -dump -dependents`), and copies the
runtime's DLLs among them from that Visual Studio's folder
`VC\Redist\MSVC\<version>\x64\Microsoft.VC<number>.CRT`. It prints the
imported names and the copied files, and it fails when an imported runtime
DLL is not in that folder. The runtime is not linked into the program
instead, because the prebuilt ONNX Runtime libraries are built for the
runtime's DLLs.

The AppImage does not hold the libraries of the system that the program is
linked against: GTK 3, WebKitGTK 4.1 with libsoup 3, ALSA, D-Bus, xcb and
xkbcommon. The AppImage of Picto 0.6.19 and earlier needed none of them. On an Ubuntu
24.04 that has only the packages `libwebkit2gtk-4.1-0 libgtk-3-0t64
libasound2t64 libfontconfig1 libxkbcommon-x11-0 libwayland-client0
libvulkan1` and what they depend on, the AppImage starts; the packaging
workflow checks that. A system with the KDE desktop usually has no
WebKitGTK 4.1. Without one of these libraries the program cannot be loaded,
so `packaging/linux/AppRun` looks for them with `ldd` before it starts the
program. When one is missing, it prints a message that names the missing
libraries and the package that holds WebKitGTK 4.1, shows the same message
with `kdialog`, `zenity` or `xmessage` if the system has one of them, and
ends with exit code 1. That path has been run only with stand-in programs
on macOS, not on a Linux system that lacks the library.

The debug bundle that `run.sh` makes, `target/Picto GPUI.app`, is not a
package. It has its own `Info.plist` inside `run.sh`, with another name and
another id, so that it is never taken for the installed app.

`.github/workflows/package.yml` builds the three packages and checks each
one (section 6, "CI"). The Alpha Gate calls that workflow for a release;
[`docs/RELEASE_READINESS.md`](../docs/RELEASE_READINESS.md) describes the
release.

## 4. How it is put together

Each area: what it owns, its main types, and the one rule to know there.
Details are in each module's `//!` documentation.

**Application and library thread** (`application/`, `backend.rs`).
`Application` runs `picto_core` on a Tokio runtime of its own, opens the
library with `picto_core::state::open_library_reporting` and answers
`dispatch("command", json)` with the core's dispatcher (`core/src/ipc.rs`).
`application/copy.rs` holds the copy rules.
`backend::start` runs one library thread on the application's own
`Library`; grid results and the sidebar are read once the database is
readable, while the core's recovery passes still run. Rule: the library is
never touched from the thread that draws.

**Session** (`session/`). The one model every view reads: the destination
list, the destination shown and its history, its results as a window over
the destination, the selection, the viewer, and library edits. It emits a
`Change` so each view repaints only for what it shows; `events.rs` turns the
core's change events into reads. A query-wide
selection (Select All, a range) is addressed through the library
(`Session::selection_target`), never through the loaded items. Rule: views
change the session through its methods; no view owns another's state.

**Shared UI** (`ui/`). One implementation of each control. The pieces every view relies on: layers
by name (`layer.rs`), floating panels (`popover.rs`), focus and tab scopes
(`focus.rs`), the context menu (`menu.rs`, `menu/`; pop-up windows on macOS,
drawn in the window elsewhere), dialogs (`modal.rs` with its `Host`, which
plays a dialog's exit and gives the keyboard back, `modal::backdrop` and
`modal::dialog_panel`, the one backdrop and panel; `confirm.rs`, `form.rs`),
the keys of every floating surface (`keys.rs`: Escape, the arrows, Enter and
Tab, taken by the innermost surface holding the keyboard), rail rows
(`rail_item`),
the text field (`text_input/`, editing as Chromium does per platform),
notifications (`notify.rs`), glass (`glass.rs`) and accessibility
(`a11y.rs`: roles, names, values, states and actions for AccessKit, built
only while assistive technology is attached).

**Window chrome and the frosted material** (`ui/window_chrome.rs`). This
file holds the options that every window is opened with (no system
titlebar), the parts of a window's frame that the app draws itself (the rim
on macOS, the minimise, maximise and close buttons on Windows and Linux, and
the resize edges on Linux), and the calls that ask the system to blur what
is behind a window.

The sidebar and the inspector of the main window can show the system's
frosted material: what is behind the window is seen through them, blurred,
under the theme's rail colour.

- The switch is in Settings > General > Appearance. It is named "Frosted
  glass sidebar and inspector" on macOS and "Acrylic sidebar and inspector"
  on Windows. It is off by default, and with it off the window is drawn
  exactly as before.
- The row is shown only where the system can blur what is behind the main
  window (`Window::is_background_blur_supported`, a change to the vendored
  GPUI). It is never shown on Linux, and it is not shown on macOS while
  Reduce Transparency is turned on in System Settings.
- Turning the switch on or off changes the main window at once. Save &
  Close or Apply stores the choice, and closing the Settings window without
  saving puts the stored choice back, as for a theme.
- The choice is stored in the library's application settings, under the key
  `frostedGlassRails` for macOS and `acrylicRails` for Windows. A library
  that is used on both systems therefore keeps one choice for each.
- With the switch on, the main window's background is GPUI's
  `WindowBackgroundAppearance::Blurred` (`window_chrome::show_rail_material`).
  On macOS that is an `NSVisualEffectView` with the material
  `UnderWindowBackground` behind the whole content, from which GPUI removes
  the system's tint and extra saturation. On Windows 11 version 22H2 and
  later it is the Acrylic system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`), and
  on Windows 10 version 1809 up to Windows 11 version 21H2 it is the Acrylic
  accent policy.
- `Shell::render` fills the sidebar and the inspector with the theme's rail
  colour at 80% opacity in the dark themes and 85% in the light themes
  (`window_chrome::rail_fill`). Each of the two runs over the window's full
  height, including its part of the titlebar. Everywhere else the shell
  draws the opaque app background, so the grid, the viewer and the screens
  look the same with the switch on or off.
- While a rail slides out of or into the window, the centre is not drawn
  under the part of the rail that is still inside the window. While the
  inspector fades out or in because the user opened or left a screen, it is
  drawn with the solid rail colour, and it changes to the material when the
  fade has ended.
- On Windows, GPUI draws text with subpixel antialiasing only in an opaque
  window. While Acrylic is on, the text of the main window is therefore
  drawn with greyscale antialiasing.
- The Settings window, the detail windows and the Library Manager stay
  opaque.

**Theme, motion, zoom** (`theme.rs`, `motion.rs`, `zoom.rs`). Colours derived from a
theme's background, contrast and text colours;
durations and easings; the interface zoom (5% steps between 50% and 200%)
applied in the vendored GPUI, so views need no change. Rule: anything that
turns window points into screen points goes through `zoom`'s conversions.

**Keys and the application menu** (`settings/registry.rs`, `keymap.rs`,
`app_menu/`, `shell/menu_commands.rs`, `shell/shortcuts.rs`). The registry
holds every shortcut as data: its id, its label, its group and its default
keys. `keymap.rs` makes the key bindings from
the registry, the keyboard preset and the user's overrides, and binds each
shortcut in the scope in which it is handled. The application menu is one
model (`app_menu/model.rs`). On macOS it
is shown as the menu bar. On Windows and Linux it is shown as a menu that a
button in the titlebar opens (`app_menu/presentation.rs`).

A command that the menu and the keys share is handled once, in
`Shell::menu_actions`, under the rule of its menu row. A menu row that has
a shortcut in the registry uses the keymap's action. A menu row without a
shortcut has an action of its own, which is handled under the same rule.
Each action is handled only while its rule holds. A
row of the menu bar is enabled exactly while something handles its action,
so the row and the key follow one rule.

The shortcuts `nav.commandPalette` and `nav.goToFolder` open the Quick
Search panel (below). Ten shortcuts of the registry are not implemented.
They are bound to no key and are not listed in Settings › Shortcuts. They
are listed in `keymap::NOT_YET`:
`file.addToLastFolder`, `edit.copyTags`, `edit.pasteTags`, `view.slideshow`,
`view.navigator`, `view.layoutGrid`, `view.layoutWaterfall`,
`view.layoutJustified`, `view.toggleTileName` and `view.toggleTileMetadata`.

**Quick Search** (`shell/quick_search.rs`, `shell/quick_search/`). A panel
480 px wide in the middle of the main window, over the dimmed backdrop that
the dialogs use. It has a search field and a list, and the user chooses a
row with the keyboard or the pointer. The list holds four kinds of rows
(actions, places, tags and items), each kind in a group under its name, with
at most five actions, five places, five tags and three items.

- It opens in three ways: the button with the two arrows in the titlebar, at
  the right of the Toggle Panels button; the shortcut `nav.commandPalette`
  (⌘T or ⌘K, Ctrl+T or Ctrl+K on Windows and Linux), which is also the row
  View › Quick Search… of the application menu; and the shortcut
  `nav.goToFolder` (⌘J or Ctrl+J), which opens the panel with a list of
  the places only.
  Both shortcuts are listed in Settings › Shortcuts and can be changed
  there. Like the other shortcuts of the main window, they do not run while
  the keyboard is in a text field, a menu, a popover or a dialog. They do
  run while the viewer is open.
- **Actions** are the commands of the shortcut registry that can run with
  the keyboard focus where it was when the panel opened. A command that has
  a row in the application menu is listed exactly when the menu enables
  that row (`app_menu::shortcut_rows`). A command without a menu row is
  listed when the key context of one of its bindings holds and an element
  handles the binding's action (`app_menu::handled`). A row shows the
  command's keys at the right. Choosing a row gives the keyboard focus back
  to where it was and dispatches the action there, as the application menu
  does. After the registry's commands come the enabled rows of the
  application menu that show no registry shortcut, such as Library
  Manager… and Check for Updates… (`app_menu::other_rows`). A row of a
  submenu has the submenu's title before its own. The rows that the system
  performs on a text field (Cut, Copy, Paste, Select All) and ticked rows
  are left out.
- **Places** are the sidebar's destinations (the system rows, every folder
  and every smart folder) and the subscriptions, which are read with the
  core's `subscriptions.list` command. A row has the icon and the colour
  that the sidebar gives the place, and the parent's name at the right.
  Choosing a place opens it (`Session::navigate`), and choosing a
  subscription opens its page on the Subscriptions screen.
- **Tags** are the library's tags, those on the most items first, each with
  the number of its items at the right. Choosing a tag opens the All
  destination with that tag as the only filter, as the row "Filter Items
  with This Tag" of a tag's menu on the Tags screen does
  (`Session::navigate_with_filters`).
- **Items** are files and collections. They are found by the library's text
  search, which is the search of the grid's search field
  (`backend::text_filter`), in the All destination. An item's row is 92 px
  high and has the thumbnail from the grid's thumbnail cache, the name and a
  line with the type, the dimensions and the size. Choosing an item opens
  the destination that lists the items with that item's media
  (`Session::open_media_matches`, which is what Find Items with This Media
  opens) and selects the item there. A collection is then opened in the
  viewer.
- A name matches the text when it contains the text's letters in the same
  order (`quick_search/matching.rs`). Names that start with the text come
  first, then names that contain the text as one piece, then names that
  contain its letters with other letters between them. In that last case
  the first letter of the text must be the first letter of a word of the
  name. The matched letters are drawn in bold and underlined. This is the
  order that the search field of the context menus uses.
- While the field is empty, the list starts with the choices made most
  recently, in a group named Recent. The 30 most recent choices are kept for each library in
  `state.json`, in the field `quickSearchRecent`.
- The keys: Up and Down move the highlight, Enter chooses the highlighted
  row and Escape closes the panel. The key that opens the panel also closes
  it. The
  highlight follows the pointer, and a click chooses a row.
- The actions, the places and the tags are lists in memory, and they are
  searched on a background thread each time the text changes. The items are
  searched on the library's thread, 50 ms after the last change of the
  text. A text of one or two characters does not search the items, because
  the library's text search needs three characters. A search that is no
  longer the newest does no work, and its answer is dropped. The list of
  places that Go to Folder opens holds at most 200 rows.
- The list is as high as its rows, between 8 and 16 rows of 26 px. The top
  of the panel stays in place when the list becomes shorter or longer.

**Grid** (`grid/`). The grid shows the items of the open destination as
tiles. These files hold its parts:

- `layout.rs` computes each tile's position and size.
- `window.rs` holds the items that the grid has loaded. The grid holds at
  most 1,500
  items of the destination at a time, and the scrollable height is an
  estimate for all of the destination's items.
- `selection.rs` stores the selection as items: chosen items, Select All
  with exceptions, and Shift ranges as spans of the order.
- `thumbnails.rs` loads thumbnails and limits how many decoded thumbnails
  are kept in memory. `rendered.rs` draws the thumbnails the core cannot
  make.
- `menu.rs` holds the rows of the context menus as pure functions.
- `drag.rs` drags items within the app and out of it.
- `pointer.rs` holds the pointer handling that the grid shares with the
  collection editor.
- `frame.rs` reads the session and computes what a frame draws, and
  `body.rs` builds the elements.
- `zoom_animation.rs` is the animation played when the zoom changes.

The zoom. Each destination saves a tile size in pixels (`target_size`), and
the saved value is used as it is. In the grid and waterfall layouts,
`layout::columns` turns the tile size into a number of columns for the
grid's current width: at least one column, and at most the number of columns
that 150 px tiles give. One zoom step adds or removes exactly one column. A
zoom step is a press of the toolbar's − or + button, a move of the toolbar's
zoom knob to the next position, a press of a zoom key (`+`, `=` or `-`), or,
when Settings > Controls > Mouse wheel is "Adjust thumbnail size", one notch
of a mouse wheel or 60 px of trackpad scrolling. After a step, the size that
is saved is the size at which the new number of columns exactly fills the
grid's width (`layout::size_for_columns`). The same window width therefore
opens with the same number of columns after a restart, and a wider window
opens with more columns. The justified layout has a target row height and no
columns. There the zoom is the row height, from 150 to 900 px. Dragging the
knob sets any height in that range, and one step multiplies or divides the
height by 1.1. The arrow keys and the page keys use the same number of
columns and the same row height as the layout (`layout::keyboard_step`).

The zoom animation. When the zoom changes and the items, the width, the
spacing, the layout mode and the text rows stay the same, every tile that is
in the viewport before or after the change is drawn moving in a straight
line from its old position and size to its new position and size. This takes
150 ms (`motion::ZOOM_GLIDE`). The subfolder tiles above the items move in
the same way. The scroll position is set so that one tile stays at the same
height on screen: the tile under the pointer when the pointer is over the
grid, and otherwise the tile nearest the centre of the viewport. A zoom step
made during the animation starts a new animation from the positions the
tiles have at that moment. A change of the grid's width, spacing, layout
mode or results is not animated: the new layout is drawn in the next frame.
The check box Settings > Controls > Grid Defaults > "Animate zoom in the
grid" turns the animation off. It is on by default. When it is off, a zoom
step draws the new layout in the next frame, and the same tile still stays
at the same height on screen.

Rule: every decoded picture leaves GPUI's sprite atlas through
`crate::atlas::release`.

**Viewer** (`viewer/`). One media surface for images, animation, Flash
(Ruffle), video and audio (`video/`, behind `video/backend.rs`:
AVFoundation on macOS, the `ffmpeg` binary elsewhere, Symphonia and CPAL for
sound) and documents (`document/`). Three hosts: the media view, Quick Look
and detail windows. Collections open in the reader or the editor. Rule:
decoding never runs on the thread that draws.

**Screens** (`screens/`). There are three: Tags, Subscriptions and
Duplicates. A screen puts its buttons in the titlebar
(`Toolbar::set_screen_controls`). The Subscriptions and Duplicates screens
build their labelled buttons with `ui::titlebar_button`: when the titlebar
is too narrow for the labels, all of the screen's labelled buttons show
only their icons, and each button's label is then its tooltip.

The Subscriptions screen draws its subscriptions as grid tiles
(`grid::layout`). The page of one subscription shows the subscription's
counters, schedule and destination in a rail. The shell draws that rail
where the inspector is drawn for the grid, with the inspector's width and
the inspector's show and hide command, and the rail is built from the
inspector's controls (`ui::panel_section`, `ui::property_row`). Any screen
can give the shell such a rail (`shell::ScreenRail`).

The Duplicates screen (`screens/duplicates/`) shows the pairs of files that
a scan found to be similar, one pair at a time. It has no rail.

- The two pictures are side by side and are zoomed and panned together. A
  picture has no border and no background around it. A picture that is
  narrower than its half of the screen is drawn against the middle of the
  screen, so the two pictures are next to each other.
- Above each picture is a row 28 px high with the file's name on the left
  and the Keep left or Keep right button on the right (keys Z and X).
- Under each picture is a list of the file's facts, drawn with
  `ui::property_row` in the inspector's words and formats: Dimensions,
  Size, Type, Rating, Tags, Date Created, Date Imported, Group and
  Occurrences. A row whose value is the same for both files is drawn in the
  secondary text colour. A row whose value differs is drawn in the primary
  text colour for both files.
- The titlebar has, from left to right: the Previous pair button, the
  position in the queue (for example "1 / 500") and the Next pair button
  (keys ← and →); one line of small text in the secondary text colour with
  the similarity and the pixel measurement (for example "100% similar · 0%
  of pixels differ"); the Smart merge (S), Not duplicates (N), Keep both (B)
  and Show Difference (D) buttons; and the Zoom to fit (F) and Actual
  pixels buttons.
- The pixel measurement says what share of the two originals' pixels
  differ. It never says that two pictures are identical. The line of text is
  drawn when the measurement is finished, and it does not change while the
  same pair is on screen. When the two files cannot be compared, because one
  of them is not a still image or could not be decoded, the line has only
  the similarity.
- When the titlebar is too narrow, its parts are removed in this order: the
  pixel measurement is cut off with an ellipsis; the four buttons Smart
  merge to Show Difference lose their labels, all at the same width; the
  line of text is removed; then the position, the two zoom buttons and the
  Previous pair and Next pair buttons. Nothing wraps to a second row.
- While the pointer is on Smart merge, or Smart merge has keyboard focus,
  the picture that Smart merge would keep has the outline that the grid
  draws around a selected tile. No outline is drawn when the library cannot
  rate one file as better.
- The pictures are replaced by the pixel difference while the pointer is on
  Show Difference, while the D key is held down, and after a click on Show
  Difference until the next click anywhere.
- The mouse wheel zooms the pictures. It never moves to another pair.
- The thumbnails and originals of the pair after the pair on screen are
  decoded in advance, and its originals are compared in advance, so that
  after a decision the next pair is drawn from its originals at once.
- The screen measures no size in one frame to use it in the next frame. The
  pictures and the titlebar's row are built for their size in the frame
  that lays them out (`ui::sized`), so the screen is drawn correctly in
  every frame while the window is resized.

**Inspector**
(`inspector/`): the destination, one item, several items, the pickers and
their dialogs.

**Shell** (`shell/`). The parts of the main window: titlebar,
sidebar, rails, toolbar, imports and exports, batch rename, permanent
delete, the Inbox review. Item menus (`item_menu.rs`, `item_menu/actions.rs`)
record what they act on when they open (`Target`: the grid's selection, or
the one item the viewer or preview shows) and perform the chosen row; keys
and application menu rows run the same rows. `GridScope` is the one
classification of the grid's destination.

**Imports, exports, opening files** (`imports/`, `export/`, `opening/`,
`prompt.rs`). Imports through the core's queue, exports through
`media.export` and `picto_pack.*`, the command line and opened files, one
instance per library (a lock plus a Unix socket or named pipe).

**Site logins** (`logins/`, `platform/login_window/`). A user signs in on
the service's own login page, in a sign-in window on the
system's web view (WKWebView, WebView2, WebKitGTK through `wry`), each on a
private data store; per-service completion rules in `adapters/`; the session
as a pure state machine (`machine.rs`). Where logins are kept depends on the
build (`logins::uses_keychain`). A release build keeps them in the system
keychain, where the core files each login under the site's name alone, so
this app and Picto 0.6.19 and earlier share one login per site. A debug or test
build sets `PICTO_EPHEMERAL_CREDENTIALS=1` in `main`
(`logins::keep_credentials_in_process`) and keeps logins in memory, where
they are lost when the app quits; agents, tests and CI only run those
builds, so they never touch the keychain. Rule: platform calls stay in `platform/login_window/`.

**Libraries** (`libraries/`). Admission (section 2), this app's list and
consents, the main window's content (`host/`: open, switch, close, rename,
move, delete, cloud restore and join, the gate and its questions), the
Library Manager (`manager/`) and the process probe (`platform/`). Rule: every
open goes through `host::request` (or `host::create`), every change on disk
through `host::rename`, `move_library` or `delete`.

**Desktop** (`desktop/`). The Inbox badge, the system notification and the
guarded file hand-offs; `mod.rs` is portable, `macos.rs`, `windows.rs` and
`linux.rs` hold the platform calls.

**Languages** (`i18n/`). Eight catalogs (`i18n/locales/*.json`, included
at build time) and the rules for using them: `t("English text")`,
`tf!("… {value0} …", a)`. The language is the one chosen in Settings, else
the system's, else English, switched live in every window. Numbers and dates
follow the system's language (`format.rs`, checked against ICU).

**Settings and state** (`settings/`, `state.rs`). One settings model from the
library's rows, with a default for every row that is missing, edited as a
draft. What is kept per machine and not per library is in `state.json` under
`Picto GPUI` in the platform's configuration folder, never in a library;
written 250 ms after the last change and on quit; an unreadable
file is set aside as `state.json.corrupt`.

## 5. Rules of the codebase

- **One implementation of each shared thing.** Use the one in section 4
  (floating panels, layers, focus, menus, dialogs and their `Host`, text
  fields, notifications, `tags.rs`, the folder tree in
  `inspector/folder_picker.rs`, `GridScope`, item actions in
  `shell/item_menu/actions.rs`); fix it rather than adding a variant.
- **Library edits** go through `session::edit_then` (`edit_then_in` when the
  follow-up needs the window) or `Session::edit_reporting`, with a `Report`
  (the failure's title, and a broad change announced with Undo) and a
`Reread` (what to read again). **Reads** go through
  `session::read_then` (`read_then_in`), or `Session::run` when nothing
  follows on a view; core commands through `Session::dispatch` or
  `Session::call`.
- **The selection is addressed, not listed.** An edit of the grid's selection
  takes `Session::selection_target`; the loaded items stand for it only
  where every selected item is loaded (names, batch rename).
- **A view whose content depends on its own size builds that content for
  its bounds in the same frame.** It returns `ui::sized` or
  `ui::sized_view` (`src/ui/sized.rs`) from `render`, or puts one where the
  content goes. That element takes part in its parent's layout, and once
  GPUI has computed its bounds it builds the content for exactly those
  bounds, in the same frame. The grid, the toolbar, the media surface, the
  collection reader and editor, the document viewers and the Tags,
  Subscriptions and Duplicates screens are built this way. A view must not
  store a size while one frame is drawn and build the next frame from it:
  the frame in which the size changes (a window being resized, the sidebar
  being hidden) would then be laid out for the size before. Bounds that are
  recorded while a frame is drawn are read only by the handlers of later
  events, for example to open a menu under a button.
- **No view is notified while a frame is drawn.** GPUI does not act on
  `cx.notify()` during a draw: the frame is shown as it was built, and no
  further frame is scheduled. A notification is made in an event handler or
  in a task. Code that runs during a draw and finds that the next frame
  must differ, such as a step of an animation, calls
  `window.request_animation_frame()`. If another view has to be drawn again
  as well, it makes the notification after the frame with `cx.defer`, as
  the text field does when its text starts to need a second line. A view
  that waits for a frame that nothing schedules looks stuck until the
  pointer moves; the off-screen tool's lazy mode catches this. A focus
  listener is the one exception: GPUI calls it at the end of a draw, after
  the frame was built. When what it changes has to be drawn, it also calls
  `window.request_animation_frame()`.
- **Pictures** leave the sprite atlas through `crate::atlas::release`.
- **Layers by name** from `ui/layer.rs`; no literal priorities.
- **Shortcuts** come from the registry through `keymap.rs`; a key outside
  the registry that differs by platform is bound with `keymap::bind_written`.
  Every registry id is bound or listed with its reason in `keymap::NOT_YET`
  (`every_id_is_bound_or_listed_as_not_yet`).
- **Platform code** lives only in platform leaves: `desktop/{macos,windows,linux}.rs`,
  `libraries/platform/`, `opening/instance/{unix,windows}.rs`,
  `platform/{macos,windows}.rs` (reached through `ui/window_chrome.rs`),
  `platform/login_window/`, `viewer/video/avf.rs` (AVFoundation) and the
  per-platform thread clock in `stats.rs`. Win32 is called through the
  `windows` crate. All three platforms are required: a feature that works on
  one only is listed in section 7.
- **Test seams are debug-build only**, read in `src/seams.rs` under
  `cfg(debug_assertions)`; the off-screen tool's variables, dialog answers
  and the record of desktop requests exist in test builds only.
- **Every shown text goes through `t`**, written as its English key in
  `src/i18n/locales/en.json` (`t("Move to Trash")`,
  `tf!("Delete {value0} items", count)`). A text that has no key there is
  `en("…")` and listed in `src/i18n/gpui_only.txt`; a key-like literal that is not shown carries
  `// i18n: <why>`. `i18n::guard::every_shown_text_goes_through_t` enforces
  it.
- **Every control says what it is**: a role and a name (its label, its
  `aria-label`, else its tooltip); a click that is not a control is
  `.a11y_presentational()`; ids do not repeat under one node. `ui::a11y::tests`
  guards it.
- **Comments describe behaviour**, with no ticket numbers. A missing
  behaviour is a `TODO:` saying what is missing, kept until it exists.

## 6. Verifying a change

### Tests

```sh
cd app
cargo test --locked
```

Most tests are pure (layout geometry, the grid's window, the context menus, the filter model, the
keymap, the viewers' arithmetic, the safety guard and detach, the state
file, the format matrix's fixtures, each shared control's accessibility
tree). The ignored ones need something a plain run lacks; run them by name:

| Test | Needs |
| --- | --- |
| `real_copy_answers_commands_and_reports_changes`, `real_snapshot_*` | `PICTO_GPUI_TEST_LIBRARY` naming a copy; they write to it |
| `window_reads_stay_flat_across_the_library` | the same; times every grid read (`--nocapture`) |
| `offscreen::plays_a_script` | `PICTO_GPUI_COPY`, `PICTO_GPUI_SCRIPT`, `PICTO_GPUI_OUT` (below) |
| `viewers_render_off_screen` | a GPU; pictures when `PICTO_GPUI_MATRIX_OUT` is set |
| `renders_a_real_file` | `PICTO_GPUI_MEDIA=<file>` (`PICTO_GPUI_MEDIA_MIME` for its type) |
| `stop_reloads_the_movie_at_its_first_frame`, `real_swf_produces_frames_and_responds_to_mouse` | a native GPU adapter |
| `real_webm_produces_frames` | `PICTO_GPUI_TEST_WEBM` and `ffmpeg` |
| `a_feed_written_by_the_package_script_describes_its_packages` | `PICTO_GPUI_TEST_FEED` naming an update feed that `package.sh` wrote, with its packages beside it |
| `decodes_and_seeks_a_real_audio_file`, `plays_through_the_output_device` | `PICTO_GPUI_TEST_AUDIO` (and a sound device) |

`PICTO_GPUI_TEST_VIDEO=<file> cargo test --locked --test avfoundation_frames`
checks AVFoundation's frames on macOS (a plain program, since AVFoundation
reports to the main thread).

### The off-screen tool

`offscreen::plays_a_script` (`src/offscreen.rs`) opens the whole app on a
copy in a headless window (Metal and the system's text on macOS; wgpu with a
software adapter and the bundled fonts elsewhere), plays a script and saves
a PNG per `shot`.

```sh
PICTO_GPUI_SILENT=1 PICTO_GPUI_MENU=window \
PICTO_GPUI_COPY=/tmp/picto-gpui-library.XXXXXX/Name.library \
PICTO_GPUI_SCRIPT=steps.txt PICTO_GPUI_OUT=shots \
  cargo test --locked -- --ignored --exact offscreen::plays_a_script --nocapture
```

The window is 1213×865 (`PICTO_GPUI_WINDOW=WxH`). Set `PICTO_GPUI_MENU=window`
on macOS so menus are in the shot. Without `PICTO_GPUI_STATE=<folder>`
nothing is kept between runs. Script lines (`#` comments and blank lines
skipped):

| Line | Effect |
| --- | --- |
| `move X Y`, `drag X Y` | pointer moved; `drag` with the left button held (window points) |
| `down X Y [clicks]`, `up X Y [clicks]`, `rdown X Y`, `rup X Y` | buttons (no modifiers; use `key` for those) |
| `scroll X Y DISTANCE` | wheel by DISTANCE points (negative scrolls down) |
| `key <keystroke>` | as GPUI writes them (`secondary-` is Command on macOS, Control elsewhere) |
| `type <text>` | text into the focused field |
| `dragfiles X Y PATH…`, `drop X Y PATH…`, `dragexit` | files from the system over the window, dropped, or taken away |
| `dropdata X Y KIND=VALUE…` | a browser's drag without files: `image=PATH`, `uri=`, `text=`, `download=`, `html=PATH` |
| `wait <ms>`, `pace <ms>` | let the app run; how long it runs after each input line (default 200) |
| `shot <name>` | save `<name>.png` |
| `window <n>` | send what follows to the nth window (0 is the main one) |
| `lazy`, `eager` | draw only when a real window would / every tick (default) |
| `notify <tone>[+] <title> \| <message>` | show a notification |
| `update <status> [darwin\|win32\|linux]`, `update dialog` | put the update state where a check would; open the dialog |
| `action <name> [n]` | perform an action by name (`action app_menu::SetRating 3`, `action shortcut::Rate 3`) |
| `invoke <command> <json>` | send a core command and print its answer |
| `focused` | print the control with the keyboard |
| `a11y <name>` | write the window's accessibility tree to `<name>.a11y.txt` |
| `answer <path>`, `answer` | answer or cancel the next system file dialog (give it first) |
| `desktop` | append the desktop requests and the clipboard's text to `desktop.log` |


Nothing reaches the desktop; video does not play off-screen. Run a script
both eager and `lazy` and treat any difference as a fault.

### Driving a real window

A debug build plays the input lines above into its windows from the FIFO
`PICTO_GPUI_POINTER` names (`src/pointer.rs`):

```sh
mkfifo /tmp/picto-pointer
PICTO_GPUI_POINTER=/tmp/picto-pointer PICTO_GPUI_BACKGROUND=1 PICTO_GPUI_WINDOW=1213x865 \
  target/debug/picto-gpui-spike /tmp/picto-gpui-library.XXXXXX/Name.library &
echo "move 320 24" > /tmp/picto-pointer
```

`PICTO_GPUI_BACKGROUND=1` opens every window without the keyboard and keeps
it drawing behind other applications (`src/covered.rs`). A covered window's
frames come back slowly from the system (up to a second), so leave 1.5 s
before a capture, and leave a frame (50 ms) between a move that reveals a
control and a click on it. A capture of one window shows neither a pop-up
menu's blur nor its pop-in scale.

### Environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `PICTO_GPUI_FPS` | every build | frame meter in the bottom-left corner (`meter.rs`) |
| `PICTO_GPUI_STATS` | every build | per-second counters and timings on standard error (`stats.rs`) |
| `PICTO_FFMPEG_DIR` | every build | the folder that holds `ffmpeg` and `ffprobe`; a program with `ffmpeg` in its own folder sets it itself |
| `PICTO_DROPBOX_ROOT`, `PICTO_GOOGLE_DRIVE_ROOT` | every build (the core's) | a folder standing in for that app's; the only cloud folders under a seam |
| `PICTO_GPUI_POINTER` | debug | input lines from a FIFO |
| `PICTO_GPUI_BACKGROUND` | debug | windows open without the keyboard and draw while covered |
| `PICTO_GPUI_WINDOW=WxH` | debug, off-screen | exact window size, not remembered |
| `PICTO_GPUI_MENU=window`, `PICTO_GPUI_APP_MENU=window` | debug | menus in the window on macOS; the in-window application menu |
| `PICTO_GPUI_STATE=<folder>` | debug, off-screen | keep per-machine state there |
| `PICTO_GPUI_INSPECTOR=<width>` | debug | inspector width |
| `PICTO_GPUI_HOLD_RUNS` | debug | Subscriptions answers runs and downloads itself, starting no network work |
| `PICTO_GPUI_HOLD_DOWNLOADS[=<percent>\|fail]` | debug | AI model downloads and update installers answered by the app |
| `PICTO_GPUI_SILENT` | debug | players and Flash start muted |
| `PICTO_GPUI_APP_DATA=<folder>` | debug | the core's application data there |
| `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>` | debug | site logins moved onto that loopback origin (`logins/`) |
| `PICTO_GPUI_LOGIN_CHECK=<folder>` | debug | sign-in window check against the app's own local login (`logins/check.rs`); writes `login-check.log` ending `result: pass` or `fail` |
| `PICTO_GPUI_NET_ORIGIN=http://127.0.0.1:<port>` | debug | update feed, search engines and site icons from that loopback origin (`net.rs`); otherwise a seam fetches nothing |
| `PICTO_GPUI_NET_CHECK=<folder>` | debug | network features checked against the app's own fixtures (`net_check.rs`); writes `net-check.log` |
| `PICTO_GPUI_UPDATE_PLATFORM=darwin\|win32\|linux` | debug | the update check follows that system's flow |
| `PICTO_GPUI_COPY`, `PICTO_GPUI_SCRIPT`, `PICTO_GPUI_OUT`, `PICTO_GPUI_LAZY`, `PICTO_GPUI_LAZY_DEBUG` | off-screen | the copy, script, output folder; start lazy; log lazy frames |
| `PICTO_GPUI_TEST_LIBRARIES=<folder>` | off-screen | start at the gate; generated libraries in that temporary folder may be listed and opened |
| `PICTO_GPUI_TEST_PICTO_RUNNING[=after-open]` | off-screen | the check for another running Picto answers that one runs |
| `PICTO_GPUI_TEST_LIBRARY`, `_TEST_WEBM`, `_TEST_AUDIO`, `_TEST_VIDEO`, `PICTO_GPUI_MATRIX_OUT`, `PICTO_GPUI_MEDIA`, `PICTO_GPUI_MEDIA_MIME` | ignored tests | inputs |
| `PICTO_GPUI_COPY_ONLY`, `PICTO_GPUI_MILLION_ONLY` | `run.sh`, `million.sh` | make the copy or the library, print its path and stop |

### CI

The job `verify:app` of `.github/workflows/verify.yml` builds and tests the
app on Ubuntu 24.04 and Windows. It plays `ci/smoke.txt` and `ci/cloud.txt`
with the off-screen tool, and it runs the sign-in and network checks in a
real window. The comment above each step says what the step checks. The
pictures and logs are the run's artifacts (`gh run download <run> -n
pictures-linux`). There is no macOS job.

The job runs for every pull request, for every push to `dev`, and in the
Alpha Gate of a release. A run on `dev` saves the build cache that the runs
of pull requests start from.

The job builds once. `cargo test --no-run` builds the test program and the
app together, and every later step starts one of those two programs
directly, so nothing is compiled a second time. The library of 2,000
generated pictures that the pictures are taken in is kept in the Actions
cache and is generated again only when a shared crate changes. The last step
runs `cargo check`, which checks that the app also compiles without the GPUI
features that only the tests switch on.

`.github/workflows/package.yml` runs `package.sh` on macOS 26, Ubuntu 24.04
and Windows and uploads the packages as the artifacts `picto-macos-arm64`,
`picto-linux-x64` and `picto-windows-x64`, which are the files a release
publishes. Each job runs `node scripts/release-audit.mjs --artifacts`, which
checks that the package folder holds exactly the files of the table in
section 3 and that the update feed gives the version and each file's size
and SHA-512. Each job then proves that its package works on that system:

- macOS: the archive is unpacked into a new folder and the disk image is
  mounted, and the program in each is run with `--version`. With the signing
  certificate, the unpacked app must also pass `codesign --verify --deep
  --strict` and hold the compiled icon catalog.
- Linux: the AppImage is unpacked into a new folder and the program is run
  with `--version`. Then the AppImage itself is run with `--version` in a
  container that holds only Ubuntu 24.04 and the libraries named in section
  3. The app's own reader checks the update feed against the AppImage (the
  ignored test above).
- Windows: the installer is run silently as a first installation, the
  folder, the registry values, the shortcuts and the file association are
  checked, the installed program is run with `--version`, the installer is
  run again as an upgrade, and the uninstaller must remove everything. Then
  the installer of Picto 0.6.19-alpha is run, this installer upgrades that
  installation, and the folder must hold this app and none of the old
  version's files.

A program that starts has found the shared libraries it is linked against,
so the `--version` runs also prove that the libraries are in the package.
The Microsoft C++ runtime DLLs on Windows are the exception: the runner has
them in its system folder, so the program would start there without the
copies in the package. For these the job's log shows which DLLs `package.sh`
copied, and the list of the installed folder shows them beside the program.
The Alpha Gate calls the workflow for every push to a release branch.
Outside a release the workflow starts by itself only for a pull request into
`dev` that changes the packaging (`package.sh`, `packaging/`, `build.rs`,
`Cargo.toml`, the signing and icon scripts, the release audit or the
workflow), because each job makes a release build. For any other change it
is started by hand.

## 7. Status

### What works

The second column says how each area was established. **Pictures**: the
same script was photographed in this app and in Picto 0.6.19, and the
pictures were compared. **Values**: numbers were asserted against the tests
of Picto 0.6.19 or against geometry computed by hand. **Tests**: the app's
unit tests. **Checks**: the app's own end-to-end checks. **Read**: the
behaviour was written from the code of Picto 0.6.19 and then seen in this
app.

| Area | Established by |
| --- | --- |
| Shell: titlebar, rails, themes, Back and Forward | pictures; values |
| Sidebar: rows, folders and smart folders, drag reorder and nesting, every menu row, smart-folder editor | pictures; values |
| Grid: three layouts, windowed results, selection, keyboard, hover preview, subfolder tiles, drag out, drop import | pictures; values |
| Grid zoom: one column per step, and the 150 ms animation of the tiles | values; pictures off-screen and of a real window on macOS, taken during the animation too |
| Filter bar and view menu | pictures; read |
| Inspector: destination, one item, several items, pickers and dialogs | pictures |
| Item menus and application menu | pictures; values; off-screen in CI |
| Quick Search panel | tests (the matching and the order of the rows); pictures off-screen on macOS in the dark and the light theme, and off-screen in CI on Windows and Linux |
| Viewer: media view, Quick Look, detail windows, Inbox review, collections | pictures; values |
| Documents and document thumbnails | tests (format matrix); pictures |
| Video and audio; Flash | pictures; seen playing on macOS; GPU tests |
| Tags, Subscriptions, Duplicates screens | pictures; commands with `PICTO_GPUI_HOLD_RUNS` |
| Imports and exports | pictures; read |
| Settings, keyboard, text fields | pictures; values; tests |
| Interface zoom | tests; pictures off-screen on all three systems; a real window on macOS |
| AI tagger | pictures; tests; a real run on macOS with downloads held |
| Log window | pictures; tests; off-screen |
| Site logins | tests; `PICTO_GPUI_LOGIN_CHECK` on macOS and in CI |
| Software updates | `PICTO_GPUI_NET_CHECK` on macOS and in CI |
| Search by Image | `PICTO_GPUI_NET_CHECK` on all three systems' web views; real searches on macOS, where TinEye, SauceNAO, Yandex and Bing gave their results page |
| Accessibility | tests; off-screen tree dumps |
| Libraries | tests (each rule); off-screen with generated libraries |
| Cloud | off-screen against a stand-in folder on macOS and in CI |
| Languages | tests; pictures in German and Japanese |

### Per platform

| | Windows | Linux X11 | Linux Wayland |
| --- | --- | --- | --- |
| Backdrop blur | none (DirectX); a more opaque glass | yes | yes |
| Frosted material behind the sidebar and inspector | Acrylic, off by default; never run | none | none |
| Context menus | in the window | in the window | in the window |
| Application menu | titlebar button | titlebar button | titlebar button |
| Inbox badge | none | Unity launcher D-Bus signal | same |
| System notification | toast | freedesktop | same |
| Copy files | `CF_HDROP` | `text/uri-list` via `xclip` | via `wl-copy` |
| Open With Other | the system's chooser | none | none |
| Drag items out | OLE `CF_HDROP`; never run | XDND `text/uri-list`; never run | GPUI's native drag |
| Promised files | virtual files; checked in CI on test data | XDS; checked in CI on Xvfb | none |
| Single instance | named pipe | Unix socket | same |
| Detail window always on top | yes | no (pin hidden) | no |
| Detail window aspect lock | not built | not built | not built |
| Video | `ffmpeg` binary | same | same |
| Package | NSIS installer; installed, upgraded and removed in CI | AppImage; started in CI | the same AppImage |
| Sign-in window | WebView2; CI check only | WebKitGTK; CI check on Xvfb | never run |

### What differs from Picto 0.6.19

This list says where the app behaves differently from Picto 0.6.19, the last
version that was built with Electron. A user who upgrades from that version
meets these differences.

- A switch closes detail, Settings and log windows; an older or incompatible
  library is refused, never migrated; New Library only makes a new folder;
  a `.library` the system hands over is refused; libraries, consents and
  the last library live in this app's `state.json`.
- In a debug or test build, logins are kept in memory and are lost when the
  app quits. A release build keeps them in the system keychain, as
  Picto 0.6.19 does.
- A disposable copy may connect to a cloud folder under its own identity and
  is disconnected at its next launch.
- Commands the application menu and the keys share follow the menu's rule:
  Restore and its key work wherever the selection is in the Trash, Accept
  and Reject wherever it is in the Inbox (Picto 0.6.19's keys look at the
  destination only).
- File › New Folder starts a rename; Organize › Tags opens the Tags screen;
  Copy in the application menu copies the selection's files.
- This app has a Quick Search panel (section 4), which Picto 0.6.19 does not
  have. It adds a button to the titlebar and the row View › Quick Search… to
  the application menu. Picto 0.6.19 registers the shortcuts
  `nav.commandPalette` (⌘K, "Command Palette") and `nav.goToFolder` (⌘J) as
  deferred and binds them to nothing. In this app `nav.commandPalette` is
  called "Quick Search", has the keys ⌘T and ⌘K, and opens the panel, and
  `nav.goToFolder` opens the panel with a list of the places only.
- The handlers for the keys of Media View (Enter), Quick Look (Space), Copy
  (⌘C) and Delete (⌘⌫) over the grid are present only while the key has
  something to act on. Without something to act on, the key press is passed
  on, as before. The Quick Search panel therefore does not list these four
  commands when they would do nothing.
- Auto Tag from a menu or its key opens centred over the window (the
  inspector's button still docks it).
- PDF, DjVu and comic pages scroll as one continuous column.
- The interface zoom reaches every window at once.
- A menu's glass lets more through than other floating surfaces; a window in
  full screen draws no rim; looping indicators draw at 30 frames a second.
- Word and PowerPoint files have no viewer.
- Keyboard moves, Home, End and the page keys go across the whole
  destination.
- On macOS Close Window hides the main window; elsewhere closing it quits.
- Failures Picto 0.6.19 leaves silent are reported (Copy, Regenerate
  Thumbnail, Set as Thumbnail, exports).
- Faults of Picto 0.6.19 that this app does not have: live translation of shortcut labels, Tab
  trapped in dialogs, the folder drag ghost on the pointer, no reorder
  marker where reordering cannot work, Export on a folder's inspector, a
  rendered cover crop shown.
- Files promised by a drag are imported on macOS (Picto 0.6.19 imports none).
- The Export dialog's formats end with ZIP Archive: the original files in
  one archive, `Picto Export.zip`, in the chosen folder (`media.export`
  with the `zip` format, which Picto 0.6.19's dialog does not list).
- Toggle Panels hides the sidebar and the inspector together or shows them
  together: when at least one is showing, every one that is showing is
  hidden, and when both are hidden, both are shown (Picto 0.6.19 flips each
  one, so one can appear as the other disappears).
- The sidebar and the inspector slide over the same 100 ms (Picto 0.6.19's
  sidebar takes 50 ms). What is between them takes its new width in the
  frame in which they start to slide, and that frame is already laid out
  for the new width (`ui::sized`, section 5). While the sidebar and the
  inspector slide, they are drawn over it.
- A picture's original is read once it has been on screen for 200 ms, in
  the viewers and the grid alike (Picto 0.6.19: 100 ms and 250 ms). In the
  viewers the same 200 ms wait applies before a video or audio player, a
  document or a Flash movie is opened, so holding an arrow key down to go
  through items opens none of them until the key is released.
- The Duplicates screen differs from Picto 0.6.19's in these ways. The
  four buttons Smart merge, Not duplicates, Keep both and Show Difference
  are in the titlebar and not in a footer, because every main screen of
  this app has its buttons in the titlebar. The pictures have no bordered
  card and no dark background around them, and a narrow picture is drawn
  against the middle of the screen and not in the middle of its own half.
  There are no "Left candidate" and "Right candidate" headings. The facts
  use the inspector's labels and formats (for example "Type: PNG" where
  Picto 0.6.19 writes "Format: image/png"), the Group row is shown for both
  files when either file is in a collection, and rows that differ are drawn
  in a brighter colour than rows that are the same. Smart merge marks the
  file it would keep with the grid's selection outline, where Picto 0.6.19
  draws a green ring around the file's card. The D key shows the pixel
  difference while it is held down, and the titlebar says what share of
  the pixels differ; Picto 0.6.19 has neither. Picto 0.6.19 stacks the two
  files in one column when the screen is narrower than 620 px and hides the
  labels of the Keep buttons when it is narrower than 900 px; this app does
  neither.
- The grid's zoom works differently from Picto 0.6.19's.
  - Picto 0.6.19 rounds the saved tile size to a multiple of 50 px before it
    counts columns. Because of that, some steps of its zoom control do not
    change the layout, and other steps skip a number of columns: in a grid
    2,500 px wide with wide spacing, 150 px gives 15 columns and 200 px
    gives 12. This app uses the saved size without rounding. One zoom step
    adds or removes exactly one column, from one column up to the number
    that 150 px tiles give, and the toolbar's zoom control has one knob
    position for each number of columns. A destination whose zoom was saved
    by Picto 0.6.19 can open with one column more or fewer than in
    Picto 0.6.19.
  - Picto 0.6.19 draws the new layout in one frame and keeps the topmost tile
    in place. This app moves the tiles from their old positions and sizes
    to the new ones over 150 ms, and keeps the tile under the pointer, or
    else the tile nearest the centre of the viewport, at the same height on
    screen. The check box "Animate zoom in the grid" in Settings > Controls
    turns the movement off. Picto 0.6.19 has no such setting. The setting is
    saved in the library's application settings under the key
    `gridZoomAnimation`.
  - In the justified layout the row height can be any value from 150 to
    900 px. Picto 0.6.19 rounds it to a multiple of 50 px.
  - The "Thumbnail size" slider in Settings > Controls goes from 150 to
    600 px in steps of 10 px. Picto 0.6.19's goes from 100 to 600 px in steps
    of 50 px. It sets a size and not a number of columns, because the
    Settings window has no grid whose width columns could be counted in.
  - When the mouse wheel is set to zoom, one notch of a mouse wheel is one
    zoom step, and a trackpad makes one zoom step for every 60 px it
    scrolls. In Picto 0.6.19 every wheel event changes the tile size by
    25 px.
- The frosted material behind the sidebar and the inspector works
  differently from Picto 0.6.19's. Picto 0.6.19 has four themes that make its
  whole window translucent: `vibrancy` and `liquidglass` on macOS, and
  `mica` and `acrylic` on Windows. This app does not draw those themes, and
  a library that stores one of them opens in the Dark theme. Instead this
  app has a switch that works with each of its six themes and makes only the
  sidebar and the inspector of the main window translucent (section 4,
  "Window chrome and the frosted material"). The switch is off by default
  and is not shown on Linux. Its choice is stored in the library's
  application settings under the keys `frostedGlassRails` and
  `acrylicRails`, which Picto 0.6.19 does not have.
- Search by Image differs from Picto 0.6.19's in four ways
  (`src/search_by_image.rs`). Sogou is not offered: its page answers every
  upload with "complete the security check and try again", and the search
  runs in a window that is never shown, so no search on Sogou gave a
  result. A file larger than 1 MiB, or of a type other
  than JPEG, PNG, GIF and WebP, is sent to the engine as a JPEG whose long
  edge is at most 1,600 pixels; Picto 0.6.19 sends every file as it is, and
  SauceNAO does not accept a 19.9 MB PNG. Yandex's page is given the picture
  as soon as it is ready to take it, 100 to 200 ms after the app presses
  Yandex's upload button; Picto 0.6.19 waits one second. TinEye is given 90
  seconds to answer where Picto 0.6.19 gives 30, because in November 2026
  TinEye took 15 to 63 seconds to search for a picture it had not been sent
  before.
- A subscription's status reads in sentence case and warns only when the
  subscription will stay broken: a run stopped on a failure and waiting for
  Retry, or a source whose last run failed for a lasting reason (the badge
  names it: Login required, Failed). A network error, a rate limit, a full
  inbox and single files that would not download are no warning; the
  Problems tab still lists them. Picto 0.6.19 warns for every open issue
  until the subscription is looked at.

### Open

Each point below has an issue with the label `area: gpui`. Issue
[#163](https://github.com/midona-rhel/picto/issues/163) lists them all.

Not done:

- Opening another library from the file manager or a second launch is
  refused (rule 4). A pack that the system opens the app with is imported
  only when a library is already open: a pack that starts the app is not
  imported, on any system. A release build that is started while Picto runs
  ends without handing its files to the running Picto (section 2, rule 5),
  so on Windows and Linux a pack opened from the file manager while Picto
  runs is not imported. Picto 0.6.19 imports it in both cases.
- The packages (section 3): until the first release that is built from
  them is published, the newest release's feed names the installer of Picto
  0.6.19, and a packaged build of this app that checks for updates would
  download and start that installer. The packages hold no license texts. No
  `*.blockmap` files are made. The installer's texts are in English
  only. The AppImage needs libraries of the system. The windows of the app
  carry no application id on Linux, so a desktop cannot match a window to
  the app's launcher entry. `.library` folders get no icon on Windows.
- Document viewers: PDF and page text selection, WOFF2 fonts, RTF character
  formatting, Markdown images and raw HTML, an EPUB chapter's stylesheet.
- Detail windows: per-image zoom memory, a loading placeholder, a menu on a
  collection's members, an item whose details cannot be read never opens
  its file, the aspect lock off macOS.
- File choosers take every file type (GPUI's prompt has no filter).
- Reduced motion is not read; right-to-left text and spell-check are not
  supported; a radio group's arrows and a select's typeahead do not work.
- GPUI cannot scale an element's contents, clip a zoomed image to rounded
  corners, or draw radial gradients, masks or blend modes; where Picto 0.6.19
  draws with those, the drawing differs.
- The DirectX renderer draws no blur.
- On Linux the sidebar and the inspector never show the frosted material.
  GPUI blurs behind a window on a Wayland compositor that has a blur
  protocol, but the vendored GPUI does not yet tell the app whether the
  running compositor has one, and X11 has no blur at all.
- Detail window always-on-top on Linux.
- Logins: FANBOX keeps only the User-Agent as its headers; password fields
  do not turn on secure keyboard entry; the OnlyFans browser login has never
  run.

Not verified:

- Anything on a Windows or Linux screen: window frames, dragging, resize
  edges, the clipboard, notifications, the badge, keyboard input, video and
  sound, drag out, drops from a browser.
- A library the user keeps opened, consented to or switched in a real
  window.
- Cloud sync against a real provider or account; network work (a real
  subscription run, gallery or model download, update install, sign-in to a
  real service). A search on a real engine has been run on macOS only.
- A screen reader on any platform.
- Promised files from Photos, Mail or a browser; files arriving through
  `application:openURLs:`; restoring a maximised window or on a second
  display.
- Pop-up menus' blur and pop-in scale on macOS, and the input method's
  candidate window in them.
- The frosted material behind the sidebar and the inspector. On macOS a
  picture of one window (`screencapture -l`) shows the two rails lighter
  than their solid colour, over a flat grey, and not the blurred picture of
  what is behind the window, so no picture shows the blur itself. On
  Windows the Acrylic code has been compiled and
  has never been run: whether Acrylic shows, how it looks under each theme,
  and whether Windows replaces it with a solid colour while the window is
  not the active one are all unknown.
- Performance: no frame-time or decode-latency benchmark has been completed.
- The packaged app in use. On no system has a package been installed and
  used with a library: the checks start the packaged program with
  `--version` only, which ends before a window opens. Not seen: the first
  start of the unsigned app under Gatekeeper and SmartScreen, the icons in
  the Dock, the taskbar and the file manager, a pack opened from the file
  manager, video thumbnails made by the bundled `ffmpeg`, an update
  installed from a published feed, and logins read from a keychain entry
  that Picto 0.6.19 made.

### Open questions

- Whether a disposable copy may connect to a cloud folder at all.

## 8. Attribution

`src/ui/text_input/` adapts GPUI 0.2.2's official `examples/input.rs`,
Apache-2.0; see `LICENSE-gpui`. The vendored GPUI crates are Apache-2.0; see
`vendor/README.md`. Sidebar icons are Tabler Icons, MIT; see
`assets/LICENSE-tabler`. Toolbar glyphs are Picto's own. Roboto and Fira Mono are bundled
under their licences (`assets/LICENSE-Roboto.txt`,
`assets/LICENSE-FiraMono.txt`). `assets/fixtures/interactive.swf` comes from
Ruffle's `tests/tests/swfs/avm1/button_goto/test.swf` at the Ruffle revision
pinned in `Cargo.toml`; see `assets/LICENSE-Ruffle.md`. The files in
`assets/fixtures/documents/` were made for this project.
