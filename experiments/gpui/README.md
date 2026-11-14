# Picto GPUI frontend experiment (#163)

A native GPUI frontend for Picto, built to reach the same behavior and look as
the Electron app. It is not a replacement yet.
Issue: https://github.com/midona-rhel/picto/issues/163

## Run safely

```sh
experiments/gpui/run.sh /path/to/Main.library
```

The launcher makes a complete private copy of the library under
`/tmp/picto-gpui-library.*/<Name>.library` and opens the app on it as an
ordinary library root, the way the Electron app opens one:

- the database through SQLite's online backup (`sqlite3 -readonly …
  .backup`), which includes committed WAL data and never touches the live
  files the Electron app may have open;
- everything else (`blobs/`, the cover files, `cloud/`, `ingest-staging/`,
  `source-runners/`, the old pre-migration backups) by APFS clone
  (`cp -Rc`), which is instant and takes no space until either side changes a
  file. The copy of a 24 GB library takes about 8 seconds. If the library and
  `/tmp` are not on the same APFS volume the launcher stops with a message
  rather than copying tens of gigabytes.

On Windows and Linux (and on macOS too), the app makes the copy itself:

```sh
cargo run --locked -- --make-copy /path/to/Main.library   # prints the copy's path
cargo run --locked -- <that path>
```

(Through `cargo run`: on Linux the binary links the application core's ONNX
Runtime libraries, `libwebgpu_dawn.so` among them, which cargo puts on the
loader's path and a bare `target/debug/picto-gpui-spike` does not find.)

It makes the same copy in a new `picto-gpui-library.*` folder of the
system's temporary folder (`%TEMP%` on Windows): the database by SQLite's
online backup from a read-only connection, everything else by the system's
own file copy, which clones where the file system can (APFS, btrfs and XFS
reflinks, ReFS block cloning) and copies in full elsewhere, so a large library
needs that much free space there. A symbolic link anywhere in the library is
refused, and a failed copy leaves nothing behind (`application::make_copy`).

Reading the library to make the copy is the only access to it. The copy holds
`.gpui-disposable`, naming the library it was copied from; the app refuses a
folder without it, the library it names, and any folder inside that library,
or one whose database is that library's file. Because the app owns every file
it can see, it writes, imports and deletes inside the copy: Delete
Permanently, Empty Trash, and the Duplicates screen's Keep and Smart merge
remove files from the copy's blobs only.

Before the application starts its workers, the copy is cut loose from work
the real library has going on (`application::detach`, again on every launch):
cloud sync is disconnected (so nothing is ever written to the user's cloud
folder), watched folders are switched off, subscription runs that were queued
or running are cancelled, and schedules have no next run, so nothing
downloads unless the user starts it here; choosing a schedule again sets its
next run. Data the application keeps outside a library (AI models) goes to
`app-data/` beside the copy, never to the Electron app's folder.

`PICTO_GPUI_COPY_ONLY=1 run.sh …` makes the copy, prints its path and stops;
the checks below use it. Delete `/tmp/picto-gpui-library.*` when done.
`PICTO_GPUI_DEMOS=1 run.sh …` adds a Media Examples menu that opens the
bundled samples of each viewer's formats; production has no such menu, so it
is hidden otherwise.

The generated macOS bundle is `experiments/gpui/target/Picto GPUI.app`.

## How the app talks to the backend

The app runs Picto's application layer, `picto_core` (the `core/` crate),
in-process, as the Electron app runs it through its napi addon:
`application.rs` sets the application data root, registers the event
callback, and opens the copy with `picto_core::state::open_library_reporting`
(`open_library`, which also hands over the `Library` as soon as the database
is open) on a Tokio runtime of its own. That starts the same workers as
production: publication, imports and ingest, derivatives (thumbnails,
perceptual hashes and the automatic duplicate scan), full-text indexing,
checkpoints, AI tagging, and the subscription scheduler and runners.
`Session::run` keeps working: the library thread borrows the application's
own `Library` (`LibraryApplication::library()`), so the database is opened
once. The sidebar and the grid read with it as soon as it is handed over,
while the application still runs its recovery passes (on a million items
they take 10–30 s); the screens' own reads and edits (`Session::run`) wait
for the whole open, as commands do.

- Commands: `Session::dispatch("command.name", json!({...}))` sends a command
  as the renderer's `invoke(command, args)` does
  (`picto_core::state::invoke`, which answers from `core/src/ipc.rs`) and
  returns a `oneshot::Receiver<Result<serde_json::Value, String>>` to await in
  `cx.spawn`. It runs on the application's runtime, never on the drawing
  thread; a command sent while the library still opens waits for it. The
  names and arguments are those of `src/platform/*Api.ts`.
- Events: the application's `library/changed` (with the resources and items a
  write touched) and `picto:thumbnail-changed` arrive on a channel the
  session drains with its replies. One poll's invalidations are merged into
  one, as the renderer's registry delivers one per frame, and the session
  emits it as `application::Event`, which views subscribe to beside
  `Change`. The session itself does what the Electron runtime stores do:
  `sidebar`, `folders`, `smart_folders`, `tags` and `duplicates` read the
  destination list and counts again (`sidebarSettle.ts`); `library` refreshes
  the inspector when the shown item is named, drops the saved grids that
  Back would restore, and reads the shown grid again in place
  (`gridSettle.ts`, `requestReconcile`): coalesced, spaced by three times
  the last refresh's time between 40 ms and a second, held while the grid
  has a press, rubber band or drag going, a destination is being read or
  the destination changes, reading the same window again with where the
  selection's items now are (`root_positions`), keeping the selection by
  item and the topmost visible item where it was on screen; `tasks` reads the import tasks
  (`taskSettle.ts`); `subscriptions` and `tasks` read whether subscriptions
  run (`Change::Activity`), every 1.5 s while one does. The Duplicates screen
  reads its queue again on `duplicates`, and the Subscriptions screen its list
  on `subscriptions` (after 250 ms of quiet) and `tasks`.
- Still straight to the library (`Session::run` or the backend's requests):
  grid windows and saved views, item details, rating, rename, notes, tags and
  folders of an item, lifecycle, undo and redo, the sidebar's folder edits,
  the settings model, the Tags screen, the Subscriptions screen's reads and
  rename, and the Duplicates screen's queue and decisions. These are the same
  `Library` calls the commands make, so they publish the same events; they
  can move to commands one by one.

Linking `picto_core` adds 166 crates to the lockfile (859 to 1,025), about
2 minutes 40 seconds to a clean debug build on this machine (1,560 CPU
seconds), and takes the debug binary from 59 MB to 126 MB. Incremental builds
of the app are unchanged. ONNX Runtime links statically from `ort`'s cache;
video work needs `ffmpeg` (`PICTO_FFMPEG_DIR`, `vendor/ffmpeg/` above the
executable, or `PATH`).

## Toolchain and GPUI version

The experiment has its own Cargo workspace and lockfile. It depends on
`gpui-pre` and `gpui-pre-platform` `=0.3.12`, the 9 November 2026 crates.io
snapshot of upstream GPUI (`zed-industries/zed`). The official `gpui` crate has
not been published since 0.2.2 (October 2025). GPUI promises no API stability
between snapshots, so the version is pinned exactly and upgraded deliberately.

Four of the snapshot's crates (`gpui-pre`, `gpui-pre-apple`, `gpui-pre-wgpu`,
`gpui-pre-windows`) are vendored under `vendor/` with a backdrop-blur primitive
added, and replace the published ones through `[patch.crates-io]`;
`vendor/README.md` lists every change against 0.3.12 so that it can be
re-applied to a newer snapshot.

The snapshot needs Rust 1.98, the version `rust-toolchain.toml` pins. Without
rustup, install that toolchain anywhere and put its `bin` first on `PATH`;
`.toolchain/` in this directory is ignored by Git for that purpose.

## Implemented

- Keyboard focus (`ui/focus.rs`, one implementation for every shared
  control): buttons, icon buttons, check boxes, switches, segments, selects,
  sliders, number steppers, date pickers, colour and theme swatches, chip
  remove buttons, read-only dialog fields and text fields are tab stops.
  In a dialog, a popover and the Settings window Tab and Shift-Tab go round
  their own stops in production's DOM order and wrap, scrolling the stop
  into view; the dialog gives the keyboard back when it closes. A stop
  shows production's `:focus-visible` ring (`.btn`: 2px primary-60, 1px
  out; check boxes, date picker, condition buttons, settings search
  results, cover candidates, steppers each their own; none where production
  draws none) only when Tab put it there. Enter and Space press a button,
  Space ticks a check box, a select takes `CmSelect`'s keys (Space, Enter,
  Up, Down, Home, End, Escape, Tab closes it), a slider an `<input
  type="range">`'s. A click leaves the keyboard where it was (so the
  window's shortcuts keep working) but the next Tab starts from the control
  clicked. The off-screen tool's `focused` line prints the stop with the
  keyboard.

- Backdrop blur behind the glass surfaces (`ui::GlassPanel`): what the window
  drew behind a panel shows through its fill blurred, at production's
  `backdrop-filter` for that surface (`--glass-blur` for panels, menus,
  dialogs, pickers, popovers, the tag results list and the return-to-top
  button; 16px for tooltips; 12px under the detail toolbar; 5px under the
  inspector's type label), and its shadow is cut out beneath it as CSS draws a
  `box-shadow`. Stacked panels blur each other, and a fading panel fades its
  blur. Metal (macOS) and wgpu (Linux) draw it; Windows' DirectX renderer
  leaves the backdrop unblurred, so there the glass fill is the more opaque
  one used where nothing blurs (`theme::Theme::glass`).
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
  choice; an item of unknown shape lays out as 3:2 and fills its tile. The
  layout runs in double precision as `layoutMath.ts` does, so near ties pick
  the same waterfall column. Badges are stroked half inside and half outside
  as a canvas stroke is; names truncate as `truncateText` measures them, and
  a selected name's pill is the text 3px wider either side. A thumbnail is
  the `.jpg`, else the `.png` (`grid::thumbnails::existing`; the grid's cache
  tries the `.png` when the `.jpg` cannot be read). While a search or filter
  narrows the grid the title reads "<scope> / Search results · N", the scope
  a link back to it unnarrowed.
- Grid selection as the Electron grid has it: nothing selected when a scope
  opens, a click selects one, Cmd-click toggles, Shift-click and Shift with a
  move select a range, Cmd+A selects all, and Escape or a click on empty
  space clears. Only the image takes the pointer, not the text rows under it.
  The hovered image shows the magnifier corner, except on video and while
  scrolling. "Return to Top" rises once the grid has scrolled 200px.
- Rubber-band selection as `CanvasGrid.tsx` does it: a left press on empty
  space (gaps and the text rows under images count) shows the square-cornered
  rectangle after 5px, selects the images whose image rect it overlaps with
  the rings updating while dragging, keeps the existing selection when Shift
  or Cmd was held at the press, follows the pointer anywhere in the window,
  scrolls within 50px of the top or bottom edge at 12 × (1 − distance / 50)
  px per frame while the rectangle keeps tracking, commits on release, and
  swallows the click that release makes.
- The hover preview of `HoverPreviewPortal.tsx`: resting 200 ms on the
  magnifier opens the original (not for video) over the whole window, within
  48px of its edges, with radius 8 and its shadow. Nothing shows until it has
  decoded and 150 ms have passed; the backdrop and image then fade in over
  150 ms. Leaving the magnifier closes it after 90 ms, any scroll at once. It
  takes no pointer events.
- Empty and error states from `GridScreen.tsx`: each scope's title and
  description, a search or filter explained first, the framed glyph of the
  shared `EmptyState` (`ui::empty_state`) with its Import Files and Import
  Folder buttons where the scope has them, and a failed query's message with
  Retry. A loading grid shows nothing.
- Tile states from `drawBase.ts`: a thumbnail that could not be read or
  decoded shows the torn page on the placeholder colour; fonts draw their
  "Aa" card instead of loading a thumbnail; audio draws a faint tint with its
  cover art in grey at half strength.
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
- Sidebar context menus with the Electron rows in their order: the
  visibility menu on system rows (which system rows and sections show),
  Clear Recently Viewed, Trash's Restore All, the Folders and Smart Folders
  headers' menus, and the folder, smart-folder, group and multi-selection
  menus. Wired through the library: New Folder (header "+", menus), New
  Subfolder, New Smart Folder Group and its child groups, each created,
  revealed and renamed in place; Rename; Duplicate; Delete, which removes
  folder and smart-folder definitions and never media; Refresh Results; Sort
  Results by Name; Expand and Collapse for a row, the whole tree or the
  selection; Clear Recently Viewed and Restore All. After an edit the
  destination list is read again; places are kept by id, and deleting the
  open folder falls back to its nearest surviving ancestor, or All.
- Rename in place (`.renameInput`): a 20px field with a 1px primary border
  on the app background, the name selected; Enter or leaving it commits,
  Escape cancels, a right-click commits. The row shows the new name at once.
  A double-click folds the branch, the Electron default setting.
- Sidebar multi-selection as `handleRowClick` does it: Cmd toggles a row,
  Shift adds the shown rows from the last one clicked in the same section,
  a plain click selects the row alone and opens it. The row states of
  `SidebarRow.module.css`: selected, context menu open (hover fill and a
  1px border), keyboard focus, a grid drop over the row, drop inside and the
  2px before/after bars.
- The sidebar's remaining menu rows, through the library: Delete asks first
  in `ConfirmModal`'s words (one folder, one smart folder or group, or the
  selection) through the shared `ui::confirm`; Sort Folders (this level or
  with descendants, A–Z or Z–A) and Sort by (name, import date, dates,
  size, notes) as submenus; Change Icon, a submenu holding the compact icon
  picker (`ui::icon_picker`: the default tile and the 115 curated icons, ten
  to a row, each with its tooltip, the chosen one marked); the colour row
  (`ui::color_picker`, `FOLDER_COLORS`) for one node or the selection; a
  smart folder's Move to... submenu (Top Level and every smart folder that
  is not the node or under it); a folder's or folder selection's Move
  to..., which opens the shared folder picker (`inspector/folder_picker.rs`)
  in its move mode at the menu's position: the Library row for the top
  level, only the folders that are neither moving nor under a moving one,
  the shared parent chosen, and Move moving the topmost folders there; Add to and Remove from Quick Access; Set
  Auto Tags...; Attach Watched Folder... and Remove Watched Folder (with
  its confirmation); Edit Smart Folder..., Edit Rules..., New Smart Folder
  and New Child Smart Folder. The visibility menu's rows stay open and their
  checks follow the toggle.
- Quick Access (`sidebarQuickAccess` in the settings model): the section
  after the system rows with its count, folded under `quick_access`, its
  rows opening and offering their menus as the tree's do.
- Drag reorder (`Sidebar.tsx`'s folder drag): a press on a folder or smart
  folder row that moves 5px drags it, or the selection it is part of; the
  row under the pointer shows before, inside or after by the top 30%, the
  middle and the bottom 30%, a node never over itself or its descendants;
  the ghost is a 200px highlighted row (active surface, border, radius 5,
  90%, shadow) under the pointer with the grabbing cursor, "N items" for a
  selection; the release moves the topmost dragged nodes and reorders the
  new parent's children (`planSidebarTreeDrop`, then `move_folder` and
  `reorder_folder_children`, or the smart-folder equivalents). While
  anything is dragged, the pointer lights no row.
- Items dragged from the grid (`dragState.ts`): a press on an image that
  moves 5px drags the selection, or that item when it is not selected,
  with `DragGhost`'s stack of up to three 44px thumbnails and the count
  badge; the rubber band still starts only on empty space. All, Inbox and
  Trash set the items' lifecycle; a folder adds them, and makes them active
  when they came from the Inbox or the Trash. The row under the pointer
  shows the hover look while they are over it. The release that ends a
  drag selects nothing.
- The smart-folder editor (`SmartFolderModal.tsx`): New Smart Folder edits
  everything, Edit Smart Folder... the name, icon (a trigger opening the
  compact picker in a popover), colour and notes, Edit Rules... only the
  rules at 760px. Rules come in groups matching all or any of their rules,
  included or excluded, each rule a field, an operator and its value
  (`ui::tag_token_input` for tags, the selects of file type, rating and
  size unit, number fields with their unit, a day from `ui::date_picker`, a
  colour from `ui::color_editor` in its popover, "between" with a second
  value),
  at most ten, with the minus and plus buttons. The model
  (`sidebar/rules.rs`) compiles to the library's `FilterExpr` as
  `queryModel.ts` does and reads it back, tested both ways. While rules are
  edited the folder is saved 100 ms after each change so the grid follows;
  Cancel writes the rules it opened with back. Enter saves.
- Set Auto Tags (`FolderAutoTagsModal`): the tags all chosen folders share
  in `TagAssignmentControl` (the label, the tags as chips with their remove
  buttons, and "Add tags" or the square add button), which opens the tag
  picker above it, over the dialog; each change is saved at once through
  `set_folder_auto_tags`. Auto-Import Folder
  (`FolderWatchModal`): the directory from the system's folder chooser,
  Enable Watch, Watch Subfolders, Import Status, Remove Watch, the folder's
  structure checked as it is chosen (`imports.folder.analyze`: "Checking
  the folder structure...", the depth warning, or the warning that it could
  not be inspected, which hold Save), sent as `folders.watch.set` or
  `folders.watch.clear`; the application's watcher imports what arrives
  there every 30 s while Settings' Auto-Import is on. A copy cut loose at
  launch has no watch until one is set in it.
- Items dragged from the grid onto a subfolder tile go into that folder.
  In a folder that shows only its own items, a drag among them puts them in
  an order of their own (`computeReorderTarget`, the blue marker, then
  `folders.items.reorder` and the folder's manual order, the items kept
  selected); a folder showing its subfolders' items has no reorder marker
  (production shows one and then fails). Items dragged out of the window go
  to the system as their files (`media.resolve_target_paths`), and are not
  imported again if they come back over it within 3 s.
- The subfolder tiles (`SubfolderGrid.tsx`) above a folder's items: "Folders
  (n)" with its disclosure, a card per child (or per descendant when the
  folder shows its subfolders' items) with its cover, glyph, name on up to
  two lines and count, then "Content (n)"; click selects (Cmd toggles), a
  rubber band over them selects them, double-click opens the folder, the
  right-click menu is `buildFolderContextMenu` (or the bulk one), and
  Rename, New Subfolder and Duplicate from it rename the tile in place. One
  tile selected shows that folder in the inspector: its own count, size and
  first items, name, notes and details (production keeps the parent's
  scope there unless an item was selected before, and then shows no covers
  or size).
- The sidebar's keyboard: from the tree filter, Down or Tab enters the
  rows; Up, Down, Tab and Shift-Tab move with the focus look, scrolling the
  row into view; Enter or Space opens it as a click does; Escape gives the
  keyboard back. ⌘⇧⌥N opens New Smart Folder and ⌘⇧R the auto-tags editor
  for the chosen or open folders.
- Hover and pressed states from the Electron styles, with their timings: icon
  buttons (hover, pressed, toggled, disabled), sidebar rows and section
  headers (title pill, chevron, add button), the library switcher's highlight,
  chips, the inspector's sections and action rows, the scrollbar thumb, and
  pointer cursors on everything that takes a click.
- The shared context menu as `ContextMenu.tsx` draws it: glass panel at the
  pointer with a 12px margin, icons, shortcut hints, separators, a 30 ms row
  highlight shared by the pointer and the arrow keys (which skip separators
  and disabled rows and stop on submenus), Enter and Escape, and its
  `menuIn` and `menuOut`: it grows in from 96% over 70 ms about the corner
  it was placed by (the pointer's, or the right or bottom edge it was moved
  in from), and when dismissed shrinks to 95% and fades over 80 ms. A
  submenu has neither and stays as it is until the menu is gone, as
  `.submenuMenu` has no animation. A press outside closes it and goes no further. Rows
  can carry a check slot, be `excluded` (struck through with a cross) or
  keep the menu open when chosen, and the owner can check or uncheck them
  while it is open. Submenus open on hover, Right arrow or Enter, beside
  their row (4px right, 3px up, flipped left at the edge), close on Left
  arrow or 150 ms after the pointer moves to another row, and take the
  arrow keys once entered. A menu of seven or more rows has the search row,
  focused on open, which filters as `searchMenuEntries` does (label start,
  label, keywords, then letters in order; submenu rows found directly), with
  "No results" when nothing matches. A menu taller than its area less 40px
  scrolls. The grid's rows are the ones this prototype can perform: open
  with the default app, reveal, copy path and name, rename, Set Rating
  (with the item's rating checked), move to trash, and restore in the Trash.
- On macOS each menu panel and open submenu is a pop-up window of its own
  (`WindowKind::PopUp`, shown without taking the keyboard, transparent so
  the panel's corners and drawn shadow show), kept 12px inside the display
  rather than the window, so menus reach past the window's edge. The
  keyboard stays with the main window, where the menu's layer takes it; the
  search field types there through an invisible twin. The menu closes when
  none of its windows has the keyboard (another application chosen).
  `ui::menu::presentation()` is the one switch; `PICTO_GPUI_MENU=window`,
  other platforms, and a pop-up window that fails to open use the in-window
  overlay.
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
  the shortcut's keys as badges, popping in and out as Mantine's `pop`
  does: 150 ms on `ease`, from 90% about its centre and 9px below its place.
- Text fields with their focus states: the toolbar search, the sidebar filter
  with its clear button, and the inspector's name and notes. One-pixel
  blinking caret, text that scrolls to keep the caret in view, and an
  ellipsis when a field is not being edited.
- Text editing in every field (`ui/text_input.rs`), as production's browser
  fields edit on a Mac: words (⌥←/→, ⌥⇧, ⌥⌫, ⌥⌦) cut where Chromium cuts them,
  with the dictionaries for Chinese, Japanese and Thai; rows (⌘←/→, ⌘⌫, and
  ↑/↓ to the ends of a one-line field); the text's ends (⌘↑/↓); paragraphs
  (⌥↑/↓, ⌃A/E, ⌃K with its kill buffer and ⌃Y); the Emacs keys
  (⌃F/B/N/P/D/H/T/O); Undo and Redo of the field's own edits on the
  registry's keys, typing gathered into runs, never reaching the library's
  Undo, as in production; double-click a word,
  triple-click the line, drags by word or line that scroll past the edge;
  a secondary click selects the word; pasted line breaks become spaces in a
  one-line field; Chromium's selection on wrapped rows, and the caret at the
  end of a wrapped row after ⌘→. Windows and Linux take their own keys
  (Ctrl and Home/End).
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
- A tile that draws more pixels than its 512px thumbnail holds shows its
  original instead, by the rule of `thumbnailPlan.ts`: still images only,
  never GIF or JPEG XL, after a quarter of a second in the viewport, decoded
  off the drawing thread to exactly the size the tile shows, two at a time,
  and let go when the tile leaves the viewport.
- The Electron grid's windowed model (`gridWindow.ts`,
  `gridController.requestWindowAt`): the grid holds one window of at most
  1,500 items, read at an exact ordinal of the destination's order
  (`Library::query_window`), first 500 at the top, then aligned to 500
  with a page before the item asked for. The scrollbar stands for every
  item from the first frame: the height is the item count times the height
  per item measured on the first 500 laid out (capped at 8,000,000 px), and
  the window is placed in it where its ordinals fall, or with the item a
  scroll asked for at the viewport's middle; the first window starts at the
  top and the last ends at the bottom. A scroll, wheel or thumb drag asks at
  most every 40 ms for the window the viewport needs (within three
  viewports of the window's end, or outside it); one read is on its way at
  a time and only the latest is shown. Items outside the window are not laid
  out and nothing is drawn for them until their window arrives, as in
  production. A new window, a zoom or a refresh keeps the topmost visible
  tile where it was. Selection is by item: items chosen one by one, Select
  All, and shift ranges that reach unloaded items as spans of the order
  (`SelectionTarget::Range`), which the library resolves. Keyboard moves,
  Home and End, the viewer's and Quick Look's steps and its "n / total" go
  across the whole destination, reading the item's window first; Back
  restores the window where it lay. Viewport-only tiles. Thumbnails load six at a time in the order of
  `thumbnailPlan.ts`: the viewport before the margin around it, nearest its
  middle first. A load for a tile that scrolled away is abandoned. Decoded
  thumbnails beyond the visible band are kept for scrolling back, up to 192,
  and one that a view is showing is never let go.
- Keyboard movement by the rules of `useGridArrowNav.ts`: arrows and WASD,
  Home and End, Page Up and Page Down, and scrolling the target into view.
- Sidebar rows fade their highlight over 50 ms and icon buttons over 150 ms.
- The inspector rail fades over 170 ms instead of sliding when a screen
  takes the window or gives it back (`data-motion='fade'`), and a viewer
  closed by going to another destination stays in the pane and fades out
  with it, as `MediaView` does inside the fading workspace surface. "Return
  to Top" waits its 30 ms `transition-delay` before rising or sinking.
- Motion with the Electron timings. Thumbnails follow the reveal contract of
  `thumbnailRevealTracker.ts`: 100 ms after entering the 500px margin, a 500 ms
  reveal in which the image fades in over its dominant colour and that colour
  then fades out. Changing destination follows `WorkspaceSurface.tsx`: 170 ms
  fade-out on `--ease-default`, hidden until the first page is ready, 170 ms
  fade-in; clicks during the fade-out retarget it.
- Inspector for the selected item: preview at the thumbnail's own size
  (at most 200px) with its format badge, colour palette (a click opens the
  filter bar with that colour in its colour filter, a right-click its menu of
  Filter by color and the seven Copy rows, its tooltip says "Copied!" for
  1.5 s after a copy), name,
  notes, source address, folders and tags as chips that can be removed, a
  rating that previews under the pointer, and the properties the Electron
  inspector lists, Export and Auto Tag. Its left edge drags between 260 and
  550px and, under the pointer or while dragged, grows to 2px in the text
  colour.
- The application menu (`app_menu.rs`, from `electron/windows/menu.mjs` on
  macOS): Picto, Library, File, Edit, Organize, View, Window and Help with
  production's rows, titles (counted ones included: "Move 3 to Trash"),
  order, separators and keys. Keys from the shortcut registry follow the
  preset and Settings' rebinding. The selection's rows are enabled by
  production's rules (`menuContext`: selection, single file, destination,
  Inbox and Trash, running imports) and perform what its renderer does
  (`shell/menu_commands.rs`); Cut, Copy, Paste, Select All, Undo and Redo go
  to a focused text field first. Close Window (⌘W) closes Settings (dropping
  its draft) or a detail window and hides the main window, which the Dock
  icon brings back; Minimize, Zoom, Toggle Full Screen (not Settings, as in
  production), Hide, Hide Others, Show All, Bring All to Front and the
  window list (AppKit's, for a menu titled Window). About Picto opens
  Settings on About. Organize ▸ Tags opens the Tags screen, which
  production's row does not (its shell has no `tags` destination).
- The Settings window (`Settings.tsx`), opened by the titlebar's Settings
  button, ⌘, and the app menu, as production opens it: a separate 900×650
  window, no smaller, with the traffic lights hidden in favour of its own
  close button, its section title and panel header moving the window. Every
  section production has, in its order and wording, with every row: General,
  Updates, Library (the library's statistics), Sidebar, Controls, Preview,
  Shortcuts, Notifications, Auto-Import, Inbox & Deleting, Subscriptions,
  Cloud (the library's `cloud_state`), AI Models and About, plus the settings
  search, which lists the matching sections as production does. The
  Shortcuts section lists the real registry (`settings/registry.rs`, ported
  from `shortcuts.ts` with its groups, descriptions, default keys and the EU
  preset), filters it, captures a new binding from the next key pressed,
  clears one, and marks a conflicting row.
- One settings model (`settings/`), loaded from the library's `setting`
  row `application` and `view_pref` row `grid:defaults` with production's
  defaults and per-field fallbacks. The window edits a draft; Save & Close and
  Apply write it back as `settings.replace` and `settings.view.patch` do (and
  clear every saved view after "Reset all views"), then the app follows it:
  the sidebar's counts, tree guides, system rows and Folders and Smart Folders
  sections, the inspector's tag prefixes, and the grid's default view and the
  `gridSpacing` fallback, which the grid reads again. Writes go to the
  disposable copy only.
- Themes (`theme.rs`, from `tokens.css`): Auto, Light, Light Gray, Gray,
  Dark, Blue and Purple, with the light themes' stronger borders, white
  control surfaces, glass and softer panel shadows. A swatch shows its theme
  in every window at once, as `previewTheme` does; Save & Close or Apply
  keep it, and closing Settings without saving puts the saved one back. Auto
  follows the system's appearance as it changes. Every window repaints.
  The Zoom Level setting is saved and, as in production, not applied.
- Shared form controls in `ui/`, each once, to the production CSS with their
  hover, pressed, focus and disabled states and timings: the Settings footer
  and inline buttons as looks of the one action button,
  toggle switch, checkbox and radio rows, select (`CmSelect`, sized to its
  widest choice, menu below or above), compact number field with steppers,
  range slider, and the window close button and rim.
- Inspector with nothing selected, describing the destination as the
  Electron scope state does: the folder card (two peeking cards over a front
  card holding the first four covers on their palette colour, or a folder
  glyph when empty), the name, read-only for All, Inbox and the other
  built-in destinations, a folder's or smart folder's name and notes, which
  save through the library's folder owners and rename the sidebar row, Items
  and Size for the whole result, Export for folders and smart folders, and a
  Details section with the search and a folder's auto-tag and watch state.
- The inspector's tag picker (`TagSelectPanel` in `OverlayShell`), from
  Add Tags, the "+" after the tag chips, a right-click in the Tags section
  and the viewer menu's Add Tags: a 540×480 glass panel docked left of the inspector at the button
  (80px below the rail's top from the viewer menu, centred when the rail is
  hidden), over a clear backdrop that closes it on a click or right-click,
  dragged by its header or footer after 5px, Escape to close, a pin that
  keeps it open without the backdrop. The search asks the library's ranked
  `tag_page` (at once when empty, 150 ms after typing otherwise, pages of
  100 loading 200px before the end), the tags the items had on opening list
  first, each visible name once; a search naming no tag offers Create
  "name" (in the browsed group). The rail has All, the tag groups in their
  preset order and colours (hidden with `showTagGroups` off), Selected and
  Starred with their counts, and folds away (the panel narrows to 340 over
  200 ms). Rows show the bookmark in the group's colour (filled when on the
  items), the star for starred tags, the name with the match in bold
  (without its group unless `showTagPrefixes`), and the count; the settings
  panel switches grid or list and the counts. A click puts the tag on the
  item, or on every selected item, or takes it off, at once; the arrows
  (and ← → in the grid layout), Enter and Tab work as in production. A row's
  menu has Filter Items with This Tag, Add to or Remove from Starred, Move
  to Group or Remove from this Group, and Copy Tag.
- The folder picker (`FolderPickerPanel` with `FolderTree`), from Add to
  Folder, the "+" after the folder chips, a right-click in the Folders
  section and the viewer menu's Add to Folder: the folder tree with its guide lines, folding chevrons and
  parent names, the search (matches with their ancestors, every branch
  open), the items' folders checked, a click that adds the items to the
  folder or takes them out, the Recent and Selected views (Tab cycles), the
  arrows and Enter, and a row's menu with New Subfolder and New Sibling
  Folder, which ask for a name in a dialog, create the folder and add the
  items to it.
- The Add Tags and Folders dialogs (`TagSelectModal`, `FolderPickerModal`
  on the shared `SelectionModalShell`: `inspector/tag_dialog.rs`,
  `inspector/folder_dialog.rs`, `ui/selection_modal.rs`), from T, F, ⌘⇧J and
  the grid menu's Add Tags and Add to Folder: a 680×614 (560×574) glass
  dialog with the search, a rail of Recent, Selected (Members) and All, then
  the tag groups with their dots and counts, and a list of check-box rows
  (the tag list virtualised to the whole count, 100 a page; the folder tree
  with its fold buttons). Clicks and Enter only mark tags or folders; the
  footer sums them ("+1, −2 · 3 files", or "N files selected") and Apply (N)
  writes them to the selection through the inspector's edit path, Cancel,
  Escape, the close button and the backdrop forget them. The arrows, Enter
  and Tab work in the tag dialog's search as in production; in the folder
  dialog Enter applies and Tab goes on to Cancel, as the browser takes it. As in production the checks start from the item the
  inspector shows or last showed, whatever is selected; Folders' Apply
  clears the selection. Recent tags and folders are kept across launches
  (see "What is kept across launches").
- After any of these edits the library is read again: the sidebar's
  destinations and counts, the tags, the item or the selection's summary,
  and the grid when its destination's results can change (Untagged, a
  folder, Uncategorized, a smart folder, a filtered or searched grid), in
  place, keeping the selection and the scroll.
- Context menus on the inspector's chips: a tag's Filter Items with This
  Tag (narrows the grid's destination to it), Add to or Remove from
  Starred, Copy Tag and Remove Tag; a folder's Open Folder and Remove.
- The source address field's list (`InspectorSourceField`): the link
  button, "+N", or a click on an empty field opens it over the field, with
  each address (open on a click), Edit and Delete with their tooltips, an
  edit field (Enter or leaving it saves, a blank address deletes it, Escape
  leaves it), and Add URL; a press outside saves and closes it. Saved
  through the library's `set_source_urls`, for one item or every selected
  item.
- Pin: while pinned, the inspector keeps the item, selection or destination
  it shows, and its edits still apply to it. Unpinned, a multi-selection is
  read again at once; anything else stays until the selection or the
  destination changes, as production's inspector waits for its live target.
- The name and notes fields expand as `InspectorField` does: focused text
  that no longer fits its line is drawn over what follows, wrapped, up to
  240px tall (then it scrolls to keep the caret in view), on the rail colour
  with its shadow, fading in from 92% over 100 ms. Shift+Enter breaks the
  line, Enter commits, Escape reverts; the arrows move between lines. This is
  the shared text input, which takes line breaks when made `expanding`.
- Overwriting the notes or sources of a selection where some items already
  have them asks first ("Overwrite notes?", "Overwrite sources?") in the
  shared dialog.
- Inspector with several items selected, from the library's selection
  summary: the stacked preview of the last six covers chosen with its
  shade, the turned cards turned and scaled, the enter and exit motion, "N items
  selected", the shared notes and sources, folder and tag chips shared by
  all (removing one removes it from every item), a shared rating that rates
  all of them, and Media and Size.
- The inspector keeps what it shows until the next state's data is there,
  whichever state it moves between; the dashed skeleton appears only when
  there is nothing to keep.
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
  once decoded, as `ImageCrossfadeFrame` swaps them. Opening a video or a
  paged document keeps the previous item's picture the same way until the
  new one has something of its own (a video its first frame, a document its
  measured pages with the thumbnail on the first), for at most 1.5 s;
  nothing is held from before the viewer opened.
- A collection opens in `GroupSurface`'s reader (`viewer/group.rs`), in the
  media view, Quick Look and the detail window alike: its members one under
  another, each in a glass frame of its own shape the column's full width
  (at least 180px tall), 16px apart and from the edges, beside the 8px
  scrollbar gutter. A member shows its thumbnail; after 200 ms in view its
  full media loads (an image's original fades in over 130 ms; a PDF, video
  or other member opens in a media surface of its own, video not playing
  by itself, under its thumbnail, which fades out over 130 ms once the
  surface has drawn its first frame or page), the last 100 kept. A PDF,
  DjVu or comic member shows all its pages inline at the column's width,
  8px apart, without its own footer or scrolling; each is measured as the
  collection opens, so its frame has its final height before it is in view
  (production shows one page fitted in the member's frame with a page
  footer). The viewer toolbar then has no zoom and
  shows "Edit group". A right-click on a member opens its menu
  (`openMemberMenu`, with the search field and "Remove from Group" above
  "Move to Trash", which moves it out into the Trash); between members, the
  collection's (Sort by Name or Size, Ungroup...). A double-click opens a
  member by itself: "Back to group" and Escape return, the counter and the
  arrows step through the members (in Quick Look and the detail window
  too). "This group is empty." for a collection with no members.
- The collection editor (`viewer/group_editor.rs`, "Edit group" or the item
  menu's "Edit Group"): the members as 180px grid tiles drawn by the grid's
  own `grid::tile_art`, names without extensions, with the breadcrumb (where
  it was opened from / the collection) in the toolbar. Click, ⌘-click,
  ⇧-click and the rubber band select members; ⌘A selects them all; Escape
  clears the selection, then closes; Delete removes them; Space opens Quick
  Look on the one selected member, stepping through the members; a
  double-click opens a member by itself. Dragging tiles reorders them, with
  the drop line and the drag ghost. The members' menu acts on the
  selection (Sort by Name or Size and the view rows included); the space
  around the tiles has the collection's menu. Edits go through the library
  with production's history and notifications; with one member or none
  left the collection closes; a failure shows in the `.inlineError` band.
- The group organizer (`GroupOrganizerModal`): "Group..." / "Merge into One
  Group..." that needs a decision asks which collection to keep, or the new
  group's name, with the collection notes and their byte count.
- Enter or a double-click opens the media view on the one selected item;
  Enter, Space or Escape close it. Originals decode off the drawing thread;
  stale loads are fenced.
- One keyboard, from the shortcut registry (`keymap.rs`): every binding the
  app makes for a registry shortcut is built from the registry's current
  keys, both slots, the EU preset and the user's overrides, and rebuilt when
  Settings applies a change; tooltips and menu rows read their keys from it
  too. Each id maps to actions in the key context of production's scope:
  `Browser` for the app-wide keys (Back and Forward on both bindings, ⌘1–⌘4
  to All, Inbox, Untagged and Trash, ⌘F into the grid's or the Tags
  screen's search, the rail toggles, Undo and Redo (with or without a
  selection; the results, the selection and the sidebar are read again
  after either), New Folder, New
  Subfolder, Rename into the inspector's name field, Copy File Path),
  `Grid` while the grid shows (movement with Shift extending, Select All,
  Escape to deselect, Enter and Space, ratings 0–5 for the whole selection,
  Open with Default App, Reveal in Finder, ⌘⌫ by scope as
  `gridRemovalAction` decides, Restore in the Trash, Accept and Reject in the
  Inbox, ⌘O to open in a new window, and T, F and ⌘⇧J, which open the
  Add Tags and Folders dialogs), `Viewer` (previous, next, fit, actual
  size, zoom, close, ratings for the shown item, Accept and Reject for an
  Inbox item, in the media view and Quick Look), `DetailWindow` (the viewer's keys except that only Escape closes,
  ⇧T and Copy File Path) and `Duplicates`. None of them fires while a text
  field, a context menu, a popover or a dialog has the keyboard; Escape goes
  to the menu, then the popover, the dialog, the viewer and the selection.
  ⌘, works in every window. The ids the app cannot perform yet are listed
  in `keymap::NOT_YET`, and a test keeps every id either bound or listed.
- One media surface, three hosts: the media view, Quick Look and the detail
  window all show items through `viewer/surface.rs`, which takes per-host
  options (navigator, resize following) and the Preview settings: an image
  opens at 100% under `imageDefaultZoom: actual`, and under
  `viewerTrackpadGestures` on macOS scrolling pans while a pinch zooms.
- Inbox review (`InboxReviewControls.tsx`, `inboxReview.ts`): while the
  media view or Quick Look shows an Inbox item, the Accept / Reject glass
  pill sits 96px above the window's bottom edge, centred, over the dialogs.
  Each half's light (two radial gradients, three inset shadows, a sideways
  fade) and the glyphs' drop shadows are drawn as bitmaps once per display
  scale (`viewer/review_art.rs`), since GPUI has no radial gradients, masks
  or filters; the light fades in to 72% under the pointer over 160 ms, the
  glyph tints over 140 ms, a press flashes the light from `brightness(2)`
  over 360 ms, the tooltips (above, with the `inbox.accept` /
  `inbox.reject` keys) and the focus ring a clicked button shows once a key
  is pressed. Z and X (the registry's keys) work in both views. A review
  moves the view to the next item at once (skipping items still saving,
  loading the next window first when the item is the last held), saves
  through `Session::set_lifecycle` or, when rejected items are deleted,
  through the item menu's permanent delete, which asks first only when it
  cannot be undone; the pill takes no input while that question is open,
  and a cancelled or failed save brings the view back to the item. The
  last item closes the view.
- Quick Look as `GridQuickLook.tsx` draws it: an overlay over the whole
  window, titlebar and rails included, on the app background, below the
  context menus. The media sits 48px from the top and bottom and 16px from
  the sides (a collection `clamp(32px, 16vw, 280px)` from the sides), with
  no navigator; the 32px close button in the top-right corner over
  `--color-surface-hover` in 200 ms, and the previous / count / next strip
  along the bottom, disabled at the ends of the results. Space toggles it
  for a single selected item (`spaceKeyAction`, or pages the grid by 90% of
  its height when that is `scroll`); Space, Enter and Escape close it; the arrows and A/D step through the results by moving
  the grid's selection, which it follows; zoom keys work as in the media
  view, ratings rate its item. While it is open the shell drops its `Grid`
  key context, so the grid's other keys are held back.
- The detail window (`DetailWindow.tsx`, `open_in_new_window`): a window of
  its own, sized by `calcDetailWindowSize` (the media's shape within 85% of
  the display's work area, at least 400×300), centred, frameless with the
  traffic lights hidden, and kept to the media's aspect ratio while
  resized, the image keeping its framing. Its toolbar (38px, the title with
  the size, "n / total" when several items were selected, zoom percentage,
  Actual size, Fit, Always on top and Close) lies over the media at 55%
  black, moves the window, shows on focus and pointer movement and fades
  out over 250 ms a second later, or at once when the window loses focus.
  Escape closes it, ←/→ step through the items it was given with the
  "First item" / "Last item" notices, ⇧T pins it above other windows,
  ⌘⌥C copies the file's path. It has the navigator. Several can be open at
  once; opening an item already open brings its window forward. A
  collection opens as `GroupDetailWindow`: its name, the pin and close
  buttons, and its members in the reader. A right-click on the media opens
  the viewer's menu on the file alone (open, reveal, new window, copy rows,
  Search by Image, More), as production's detail window has it.
- Opening triggers per the Controls settings: a double-click opens the
  media view, or the default application under `gridDoubleClickAction:
  external` (media only); a middle-click opens the tile in a detail window
  under `gridMiddleClickAction: new_window` (the default); ⌘O opens the
  shown item with the rest of the selection; the grid menu's "Open in New
  Window" row sits after Reveal in Finder, as `buildEntityOpenContextEntries`
  places it. Production's grid menu has no "Quick Look" row, so none is
  added.
- Every viewer records the item it opens for Recently Viewed
  (`useRecordMediaView`, `record_recent_view`); the media view and Quick
  Look do not while Recently Viewed itself is being browsed, as production
  passes `recordItemId: null` there.
- The Duplicates screen, from `DuplicatesScreen.tsx`: the review queue of
  `picto_library`'s duplicate candidates, one pair at a time, side by side
  with each file's facts (resolution, size, format, rating, tags, dates,
  group, occurrences) and the similarity, the footer of decisions, and the
  titlebar's counter, Zoom to fit, Actual pixels and previous/next pair in
  place of the grid's controls. A pair goes on screen only once its details
  and both thumbnails are ready; the originals then fade in over 130 ms.
  Zoom and pan are linked between the two previews (wheel about the
  pointer, drag, double-click to fit), and Show Difference shows the
  difference of the two images while hovered. "Not duplicates" and "Keep
  both" record the pair as different files; Keep left, Keep right and Smart
  merge keep one file and merge the other's item into it (the freed file is
  deleted from the copy), with Smart merge's green ring around the survivor
  while it is hovered, fading over 140 ms. Each keeps the queue's place and
  updates the sidebar count; ←, →, F, ⌘0, N, B, Z, X and S work as in
  Electron. "Scan library" and Retry run the application's scanner
  (`duplicates.scan`, distance 16) and read the queue again, with the
  indeterminate 2px progress bar (`ui::progress_bar`) after 200 ms; a scan
  the media worker makes by itself refreshes the queue. Below 900 and 620
  points the layout tightens and then stacks as the container queries do. Loading message after 200 ms, and the empty
  states for an empty queue, a finished review and an error.
- The Subscriptions screen, read from the library: the grid of 180px cards
  (cover framed by its stored crop, name, status badge, item and source
  counts) under a header with the count, any gallery downloads and the
  Add, Add Gallery and Accounts buttons; click, Cmd-click and Shift-click
  selection, Escape to clear, double-click or Enter to open, and the single
  and bulk context menus. An opened subscription is a place: Back returns
  to the grid and the scope title links back to it. Its workspace has the
  rail (cover, name, status with the failure as its tooltip, actions, run
  progress, run counts, last check, schedule, posts per run, destination
  folders and tags) and the Sources, History and Problems tables, with the
  sticky header, hover rows and row buttons. Opening a subscription
  acknowledges its open issues (`subscriptions.issues.acknowledge`), so its
  badge stops warning, as `refreshDetail` does. The schedule (the rail's
  select and the menus' Schedule submenu, for one or several) and the
  posts-per-run field save through `subscriptions.schedule` and
  `subscriptions.posts_per_run`. Badges and table headings keep their
  letter spacing (`ui::tracked`, which draws a shaped line glyph by glyph
  with the spacing CSS adds). Managing subscriptions goes through the
  application's commands, as `subscriptionsController` sends them: Add
  (`NewSubscriptionDialog`, made on Settings' default schedule and run
  limit, then opened), Add Gallery (`subscriptions.gallery.start`, the
  service following the address typed; a download's progress fills once
  the site reports its size, and a settled one is cleaned up with
  `subscriptions.gallery.cleanup`), Rename (`RenameDialog`, from the menu or
  a double-click on the rail's name), Set Cover Photo… (the candidates 200
  at a time, then a square dragged and zoomed into place), Reset Sync
  History… and Delete (one, several, or ⌘⌫ on the selected cards) after
  their confirmations, destination folders and tags added through the
  folder and tag pickers and removed with their chips, and the add-query
  bar and each source's Run, Group, Hold, Edit (`QueryEditModal`), Delete and
  details (a double-click). A source whose site cannot run without a login
  shows production's warning. Enter in a dialog's field presses its primary
  button.
  With `PICTO_GPUI_HOLD_RUNS` set the screen writes each command it sends
  to the log and answers runs and gallery downloads itself instead of
  starting them. The reads are the application catalog's own, run through
  the library's consistent auxiliary read; the list and the query rows use
  `core/src/subscription_views.sql` and `subscription_query_views.sql`
  directly.
- Running subscriptions, through the application's runners: the rail's Run
  now, Pause, Resume and Retry (`subscriptions.run`, `subscriptions.run.pause`,
  `subscriptions.run.resume`) and Stop, Hold and Release
  (`subscriptions.cancel`, `subscriptions.hold`), the same rows of a card's
  menu, Run, Hold and Release for several, and a gallery download's pause,
  resume and stop. A subscription's controls wait while its command is on its
  way. The screen follows the run through the application's events and reads
  progress every 1.5 s while a run goes (and for 5 s after one is started).
  The sidebar's Subscriptions row shows the activity button of
  `SidebarRow.tsx` while subscriptions run or are all held: the 8px primary
  dot pulsing over 2.4 s, a pause glyph under the pointer on a 30% primary
  fill, and while everything is held a pause glyph that becomes play; a
  click holds or releases every subscription (`subscriptions.pause_all`).
- Permanent deletion everywhere production offers it: the item menus'
  Delete Permanently, ⌘⌫ in the Trash, Reject with "Delete" chosen (menu and
  key), and the sidebar's Empty Trash ("Empty Trash", "Delete All"), all
  through the shell's one `requestPermanentDelete` flow. The files a delete
  that cannot be undone frees are removed from the copy's blobs by the
  application's media worker, as in production; the source library is never
  touched.
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
  and unstar and group colours (through the settings model, which writes
  them to the library's application settings), copy a tag's name, and "Filter Items with This Tag", which opens
  All filtered to the tag as a step of history.
- Dialogs as `GlassModal` draws them (`ui::modal`): backdrop, glass panel,
  title bar with its close button, body and footer, `modalIn` (150 ms: it
  fades in, grows from 97% about its centre and rises 4px) and `modalOut`
  (120 ms, the reverse) with the backdrop's 150 ms fade in and 120 ms fade
  out, Escape and a click on the backdrop to close. The
  tag editor, merge picker, group dialogs and confirmations use it, with the
  shared action button (`actionButton.module.css`) and dialog fields.
- Context menus have a row of colour swatches (`ColorPicker`), used by the
  Tags screen's group menu and the sidebar's menus, in both presentations,
  and can hold a view of their owner's (`menu::view`), as the sidebar's
  Change Icon submenu holds the icon picker and the view menu and filter
  editors hold their panels (padded as `.customItem`). Rows can carry a
  right-click action (`contextAction`) and be replaced while the menu is
  open. A select in a pop-up menu opens its list as a pop-up window of its
  own at the button (`select::pop_up_list` places it on the display), which
  closes with the menu, so the grid's tile and empty-space menus and the
  titlebar's View options stay pop-ups that reach past the window; only the
  filter editors, whose fields take the keyboard and whose calendars open
  over the window, draw in it (`Menu::in_window`). A menu taller than its
  area shows the app's scrollbar, whose 8px gutter narrows its rows. The
  "Display" submenu is as wide as its widest toggle row.
- The view menu of the titlebar's "View options" button (`GridViewMenu.tsx`),
  at the button's left edge 4px below it, the button lit while it is open:
  Layout (Waterfall, Grid, Justified, with their glyphs), Sort by with the
  direction pill (hidden for folders and Recently Viewed, the pill hidden for
  Random), and the toggles Show Name, Show Resolution, Show Extension, Show
  File Type, Show Item Count, Compact, Fit Thumbnails (Grid only), Show
  Subfolder Content (folders), Show Sidebar and Show Inspector. Each choice
  shows at once and is saved to the destination's `view_pref` row through the
  library's `update_auxiliary_json` as `settings.view.patch` does, out of
  undo; a page still on its way keeps the change. A new sort or the subfolder
  toggle fetches again behind the scope fade. When the layout changes (zoom,
  spacing, mode, labels, width) the topmost visible tile, a selected one
  first, stays where it was on screen (`resolveGridScrollAnchor`). Show
  Extension adds `.EXT` to the names.
- The filter bar (`GridFilterToolbar`), toggled by "Filter library", whose
  button is lit with a count while the bar is open or any filter is set. It
  sits over the grid aligned with the titlebar's panel edges, with a control
  per pinned or set filter: Color, Tags, Folders, Rating, Type, Date
  Imported, Date Modified, Resolution, Duration and File Size, each showing
  what it holds, cleared by a right click or its corner button, and opening
  its editor 4px below it. The menus: ratings and types with check marks
  (types from the library's facets for the other filters, families first, a
  right click excluding), date presets with a From/To calendar
  (`DatePickerButton`), min/max ranges with their units, the colour editor
  (`ColorFilterEditor`: saturation square, hue strip, tolerance, presets,
  hex code, 250 ms settling), the pin menu, and saved filters (save, apply,
  right click to delete). Tags and folders open the inspector's tag and
  folder pickers in their filter mode (`onApplyTagFilter`,
  `onApplyFolderFilter`), below the control: Match any/all/exactly in the
  header, a click to include, a right click to exclude (Shift-click takes a
  range of folders), a check box on each row, one column of tags and no
  Create row, and the Select/Exclude hints in the footer. The
  filters combine as `compileGridQuery` builds them; the lock keeps them on
  navigation, otherwise a destination opens unfiltered and Back restores the
  filters it had; Clear empties them. The Tags screen's "Filter Items with
  This Tag" puts the tag in the bar.
- The item context menus of `gridContextMenu.tsx`, built by pure functions in
  `grid/menu.rs` (tested against the cases of `gridContextMenu.test.tsx`):
  a tile's menu for one item, several, a group, Inbox (Accept and Reject
  first), Trash (Restore, Delete Permanently) and a folder (Remove from
  Folder, Sort by, Set as Folder Cover), with production's order,
  separators, icons, shortcut hints (the user's own bindings, none for
  deferred shortcuts), labels with counts and disabled rules; empty grid
  space's menu (New Folder, New Smart Folder, imports, Sort by in a folder,
  the view options); the viewer's (`useViewerEntityContextMenu`, Flash
  playback rows first); and the inspector preview's. A right-click inside
  the selection keeps it, outside selects the tile, on empty space opens the
  empty menu. Wired through the library: open with the default app, reveal,
  copy path, name, link, the selection's paths, names and links, Accept,
  Reject (to the Trash, or deleted when the setting says so), Move to Trash,
  Restore, Delete Permanently (after `ConfirmModal`'s question, only on a
  marked disposable copy), Add to Folder's last folder, New Folder (with
  Selection) renamed in the sidebar, Remove from Folder, Sort by, Set as
  Folder Cover, Group (with the group organizer when it needs one), Edit
  Group, Ungroup (after its
  question), Rename (in the inspector's name field), Copy Tags and Paste
  Tags (an in-app tag clipboard, and the JSON on the clipboard), Set Rating
  for any selection, and the view options as production embeds them: the
  titlebar view menu's layout and sort panel inline and its display panel
  inside "Display" (the same `shell::view_menu` panels), saved to the
  destination's view as production saves them. A confirmation dialog (`ui::confirm`) for the shared `ConfirmModal`.
  Also: Copy (the files on the macOS pasteboard, from the menus, ⌘C over
  the grid, the viewer, Quick Look, a detail window and the collection
  editor, and the Edit menu), Open With Other (the applications Launch
  Services lists for the type, with their icons, the default first, looked
  up once per extension), Batch Rename (`BatchRenameModal`, also the Edit
  menu's row and its keys), Regenerate Thumbnail(s) (as the Electron main
  process regenerates, also ⇧⌘T and the Edit menu), Grayscale (a checked
  row that keeps the menu open, and ⌘⌥G), Find Items with This Media (the
  media-matches destination, with no sidebar row), Set as Library Cover
  (the shared cover dialog, `MediaCoverDialog`, framing the item at once;
  saving writes `.picto-library-cover.*` beside the copy's database, not
  production's global config), Set as Thumbnail (a video's or Flash
  movie's current frame), and the Duplicates cards' menu (Open with
  Default App, Open in New Window). Opening, revealing, Open With and file
  copies go through `desktop.rs`, which in the off-screen tool writes them
  to `desktop.log` (a `desktop` script line) instead of reaching the
  desktop.
- Importing (`filesController.ts`, `GridScreen.tsx`'s drop handler), into
  the grid's destination (the Inbox's lifecycle when the Inbox is open, else
  the library's, and the open folder): files and folders dragged from Finder
  onto the window, which shows `.dropOverlay` over the grid's area (the
  primary ring and tint, "Drop files to import", "into current folder" in a
  folder) while they are over it, and imports them wherever they are let
  go, except while the viewer, Quick Look or a screen shows or in a
  media-matches destination; Import Files... and Import Folder... (the
  empty grid's menu, the empty states' buttons, the File menu, ⌘I and ⇧⌘I),
  which open the system's chooser; the sidebar's Import Folder... (top
  level) and Import Folder Here...; and Paste Import (the empty grid's menu
  when the clipboard holds files or a picture, and ⌘V in the grid outside
  the Trash, smart folders, Recently Viewed and media matches), which writes
  a copied picture to `<temp>/picto-clipboard-imports/` and has the import
  delete it once taken. One folder opens `FolderImportModal` (Preserve folder
  structure, Include subfolders, Include folders without media, Watch this
  folder, with `imports.folder.analyze`'s depth warning); several files under
  `multiFileImportBehavior: ask` ask "Import as Collection?". Everything goes
  through `imports.enqueue`. While an import is open, another is refused
  and the open one comes forward; the import rows and buttons are disabled.
- The task dialog and task bar (`TaskDialog.tsx`, `imports/`): an import
  takes the foreground with its title, phase, rate, progress (a share that
  moves over 250 ms, or the sweeping segment while files are found), counts,
  time left, Pause or Resume and Stop…; Stop and Close ask whether to keep
  or remove what was imported (pausing meanwhile); Minimize or Escape send
  it to the task bar above the bottom edge between the rails, whose row
  shows the task's glyph on its tone, the title, progress, "done / total ·
  time left", Pause or Resume and Show; an ended import shows its result,
  "Show N failed" with the files and their errors, and Done. The list is read
  on `tasks` changes (150 ms coalesced) and every second while one works; a
  task left running when the app closed takes the foreground when it
  resumes.
- Exporting (`filesController.ts`, `ExportModal.tsx`, `PictoPackModal.tsx`,
  `export/`): Export Originals... (the system's folder dialog, then every
  file as it is, through `media.export`), Export As... (the Export dialog:
  destination with Browse, format select, quality slider for JPEG, WebP and
  AVIF, the optional width and height that take digits only, Keep Aspect
  Ratio) and Export as Picto Pack... (the pack dialog, then the system's save
  dialog with a safe file name, through `picto_pack.export`), from the item
  menu's Export submenu (the grid's and the viewer's), the inspector's
  Export (an item, the selection, a folder or smart folder), the sidebar's
  Export submenu on folders and smart folders, the File menu, ⌘E and ⇧⌘E. A
  selection after Select All or a shift range exports all of it, loaded or
  not (`Session::target_for`); a folder or smart folder exports its query.
  Picto Packs are imported from a drop of one pack (others mixed in are
  refused) and the File menu's Import Picto Pack...: `picto_pack.inspect`,
  the dialog with what the pack holds, then `picto_pack.import` while the
  dialog says "Working...". An import that is open blocks it, as it blocks
  other imports.
- Thumbnails an import is still waiting for: a tile whose thumbnail file
  does not exist asks the application to render it
  (`media.render_thumbnail_now`, as the media protocol does on a miss), and
  every `picto:thumbnail-changed` makes the grid read that thumbnail again.
  What the media worker cannot make is drawn here as the Electron main
  process draws it (`grid/rendered.rs`): a PDF's, DjVu's or comic archive's
  first page within 800 points at two pixels a point, and a Flash movie
  five seconds in, muted, written beside the library's thumbnails as
  `<hash>.png`, only in a marked disposable copy.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).
- Flash's controls (`FlashControls.tsx`): play or pause, stop (the movie
  reloads at its first frame, held), and the volume button with its slider
  that opens above it on hover (`VolumePanel`), in the shaded bar of
  `VideoPlayer.module.css` with the media player's buttons, which hides 2.5 s after the pointer stops
  moving over the movie and fades over 300 ms. The buttons stay dimmed
  until the first frame arrives. The viewer menu's Play/Pause, Stop and
  Mute rows act on the movie and show its state.
- One renderer per media type, chosen as `detailRendererKind.ts` chooses
  (`viewer/document/kind.rs`), in all three hosts (media view, Quick Look,
  detail window):
  - PDF pages rasterized by hayro (pure Rust), DjVu pages by djvu-rs (the
    crate production runs as WebAssembly) at `max(96, dpi)`, and CBZ pages
    (the archive's images in natural order), on a thread of the document's
    own, at the size they take on screen times the display's scale. Unlike
    production, which shows one page at a time, the pages are one
    continuous column (`viewer/document/paged.rs`): every page is measured
    when the document opens, each is as wide as the viewport's content box
    up to its natural size, 16px apart, 24px from the edges, on
    `rgb(42, 42, 46)`, white with its shadow. Only the pages in view and
    one each way are drawn, two more each way are kept; a page not drawn
    yet is a blank sheet of its final size, and the first page shows the
    item's thumbnail until drawn, so nothing moves. A drawing for an older
    zoom arriving late never replaces a newer one. The titlebar-high footer
    shows "Page n of m" for the page in the middle of the view (no
    Previous/Next buttons); J and L (the registry's page keys) scroll to the
    previous and next page's top, and Home, End, Page Up, Page Down and
    Space / Shift+Space scroll the pages in the media view and the detail
    window (Space there no longer closes the media view). The viewer
    toolbar's zoom drives PDF pages as production's renderer zoom does: the
    percentage and its menu, the slider, Fit (`` ` ``, fitted to the
    width), Actual size (⌘0, 100% = one point per pixel), and `+`/`-` in
    1.2× steps between 25% and 800%, keeping the place at the top of the
    view. A decoder that panics on a page fails that page only.
  - The document shell (`DocumentViewerShell`), for EPUB and the other
    reflowed documents: the scrolling viewport on `rgb(42, 42, 46)` with
    24px around the page, and the titlebar-high footer with Previous page,
    "Page n of m" and Next page, whose J and L come from the shortcut
    registry.
  - Text, Markdown (GitHub flavoured: tables, strikethrough), JSON (pretty
    printed) and RTF on the 816px text page, and EPUB chapters (the spine of
    the package document, one chapter to a page, its pictures decoded) on
    the 760px book page in Georgia, laid out as GPUI text with the page's
    heading, paragraph, quote, code, list, link and rule styles. As in
    production, the app stylesheet's `* { margin: 0; padding: 0 }` applies:
    only the text page's headings have margins, lists hang their markers
    outside the text, a book's quote is not indented, and a table's columns
    are as wide as their widest cell (header cells are not bold yet).
    Parsing runs off the drawing thread.
  - Fonts (TTF, OTF, the first face of a collection, and WOFF, unpacked)
    loaded into GPUI's text system: the title in the font, the Preview,
    Waterfall, Glyphs and Information modes, the five paper colours, and the
    floating size control (50–300%, also ⌘= / ⌘−, ⌘0).
  - JPEG XL decoded by jxl-oxide (the decoder `core/` uses) onto the image
    camera, so it zooms, pans and shows the navigator as images do.
  - Every other type, Word and PowerPoint included by decision: its
    thumbnail, "Preview is not available for <type>." and Open with Default
    App.
  - The Media Examples menu (with `PICTO_GPUI_DEMOS=1`) has a sample of
    each (PDF, DjVu, CBZ, EPUB, text, Markdown, JSON, RTF, a font, JPEG XL)
    next to SVG and Flash.
- Animated GIFs and animated WebP play in the viewer (GPUI's image element
  animates a multi-frame image that has an id); the grid keeps the still
  thumbnail, as production does.
- Video and audio play in the viewer's media player (`viewer/video/`, from
  `VideoPlayer.tsx` and its parts): the picture fitted on black, or for audio
  the visualizer of the Preview setting (spectrum, oscilloscope, orbit or
  none) on the app background; the controls over a dark gradient that hide
  2.5 s after the pointer stops and fade over 300 ms, held while the
  scrubber is dragged; the scrubber with buffered and played ranges, 4px
  growing to 6px under the pointer, and the time under the pointer (for
  audio, the thumbnail's waveform, dim ahead and bright behind, with a
  cursor); play and pause, 5 s skips, the time, the speed menu (0.25× to
  3×), loop, the volume slider over the mute button (shown on hover, gone
  300 ms after), and the volume display the arrow keys bring up for 800 ms.
  A click on the picture plays or pauses. The Video shortcuts of the
  registry (K, J, L, ↑, ↓, M, O, ], [, `\` and F, or their EU preset and edited
  keys) work while the media view, Quick Look or a detail window shows the
  player, as `keymap::Scope::Media`. As `MediaView` has it, video starts with the
  Preview settings' autoplay and loop and muted, audio plays at once, loops
  and has sound; volume starts at 90% and speed at 1× for every item.
  Playback failures are written to standard error in production's
  `media.playback` diagnostic shape. A video draws nothing (not even its
  black) until its own first frame, which is the first thing seen in its
  place: production shows the item's thumbnail first, a frame from inside
  the video, and then jumps to the start. A paused AVFoundation item is
  primed with a seek to the start, so its first frame arrives too.
- Decoding never runs on the drawing thread, behind one small trait
  (`video/backend.rs`). On macOS, QuickTime and MPEG-4 play through
  AVFoundation: an `AVPlayer` decodes in hardware, plays the sound, keeps
  sync, seeks exactly and changes speed with the pitch kept, and an
  `AVPlayerItemVideoOutput` hands over CoreVideo buffers that GPUI's
  `surface` draws without a copy. GPUI's surface shader takes only
  full-range 4:2:0 and converts with the BT.601 matrix, so the item plays
  through a video composition that writes BT.601 (and applies a track's
  rotation). WebM, Matroska, and any file AVFoundation refuses, play
  through the `ffmpeg` binary Picto already bundles for thumbnails
  (`vendor/ffmpeg`, found as `core` finds it): raw BGRA frames uploaded as
  images, following the clock of the sound. Audio files decode in-process
  (Symphonia: MP3, WAV, FLAC, Ogg Vorbis, AAC, AIFF) and play through CPAL,
  the output Ruffle uses; speed changes keep the pitch (WSOLA time
  stretching), and the visualizer reads the samples being played through a
  port of the Web Audio analyser.

- Notifications (`ui/notify.rs`, `NotificationHost`): one at a time,
  centred 76 points down, above every layer; a new one replaces the one
  shown, each leaves after four seconds or on its close button, appearing
  at once and rising 48 points while it fades over 400 ms. Error, warning,
  information and success tones with production's glyphs and colours, an
  optional underlined button, and Settings › Notifications deciding which
  pop up. `ui::notify::error(title, message, cx)` and its siblings work
  from any `cx`. Failures production reports are reported with its words
  (`Session::edit_reporting` and `session::Report` give an edit its
  failure title or its announcement); Undo and Redo say what they did
  ("Undone — Moved 3 items to Trash", with Redo), and the broad changes
  production announces (deleted folders, smart folders, items, tags,
  rejected items, grouping) say so with an Undo button (`history.rs`).
  `desktop.rs`: the dock badge shows the Inbox count, and "Subscriptions
  finished" is a system notification when a batch of runs ends (macOS;
  it needs the app to run from a bundle, and follows the pop-up switch and
  the Information tone, which production's does not). On Windows the
  notification is a toast through GPUI under the app's identity, and there
  is no badge (Electron's `setBadgeCount` has none there); on Linux the
  notification goes through GPUI's freedesktop notifications and the badge
  is the Unity launcher's D-Bus `Update` signal. Neither has been seen on
  those systems.

### What is kept across launches

What production keeps per machine (the renderer's localStorage and the main
process's `config.json`) is kept in one JSON file of this app's own,
`state.json` in the platform's configuration folder under "Picto GPUI"
(`~/Library/Application Support/Picto GPUI/` on macOS), never in the
Electron app's folder and never beside the disposable copy, so it outlives
the copy (`src/state.rs`). It is read once at start and written 250 ms after
the last change through a temporary file renamed over the old one, and at
once on quit. A missing or unreadable field takes its default and leaves the
others; an unreadable file is set aside as `state.json.corrupt`; fields of a
newer version are written back untouched. It holds:

- the main window's normal bounds and whether it was maximised
  (`windowState.main`), placed where it was only while that still overlaps a
  display, else centred, and never smaller than 700×500; a window closed in
  full screen opens at its normal bounds, as production's does.
  `PICTO_GPUI_WINDOW` wins over it, and a window opened with it is not kept;
- the sidebar's and the inspector's widths and whether each is hidden;
- the folded sidebar sections and nodes (`picto-sidebar-collapsed`);
- the filter bar's pinned fields and saved filters;
- the tag picker's layout and counts, which every picker takes when it
  opens;
- recent tags, and recent folders by the library the copy was made from;
- the keyboard preset and the rebound shortcuts (the keymap is installed
  with them before the window opens), the language and the audio
  visualization.

What production keeps in the library (the settings, each destination's view
including its zoom) stays in the library through the settings model. The
grid's zoom is saved to its destination's view 500 ms after the slider, the
buttons or the zoom keys last moved it. As in production, the app starts on
All each launch, and the destination, its scroll and selection, the detail
window's pin and the inspector's pin are not kept.

The sidebar is resized by dragging its right edge, between 220 and 500
points, as the inspector is by its left edge (260 to 550); both edges are one
element (`shell/rails.rs`).

## Not yet equivalent to Picto

- Keyboard focus departs from production on purpose where production is
  broken: production's window-wide shortcut handler swallows Tab on every
  control that is not a text field (and toggles both panels, behind an open
  dialog too), and sends Enter, Space and the arrows to the grid or viewer
  behind a dialog; nothing traps focus in a dialog. Here Tab cycles inside
  the dialog and the keys act on the focused control. A radio group is one
  stop but its arrows do not move the choice; a select has no typeahead.
  Outside dialogs, Settings and popovers, Tab still toggles the panels.

- Text fields: a line that starts with right-to-left text is laid out
  right to left (production keeps the field left to right) and its
  selection is not drawn; there is no spell-check underline; Home and End
  in the inspector's fields move the caret where production's only scroll.
- A whole destination selected names no collection count in the item
  menu's Ungroup row ("Ungroup Collections..."); production looks the count
  up. Open With Other lists the applications on macOS only; on Windows it
  opens the system's chooser (`rundll32 shell32.dll,OpenAs_RunDLL`, as
  production; not run here), and Linux offers none, as production.
- Inbox review: a review key held down is told
  from its repeats by its release or the next key, since GPUI hands an
  action no repeat flag (the off-screen test sends no key releases, so it
  sees the same review key twice in a row as held). The press is taken at
  once rather than after the next item's image has decoded (`ready`):
  the viewers show the next item's thumbnail at once. After a review the
  grid's results are read again from the first page, so a review that
  advanced past the first page loses its place.

- Tags screen: the editor's
  name field is dimmed while an edit runs but still takes typing; Undo does
  not reach tag edits; tag and group changes made elsewhere show when the
  screen is opened again, not live.
- Duplicates: the progress bar's position over a shown pair is a
  2px row above the cards (the Electron `.scanProgress` class has no rule).
  The Smart merge ring follows the pointer only, not keyboard focus. The difference
  picture is computed from the decoded images (GPUI has no blend modes), at
  most 4096 pixels on its longest side, from the thumbnails until both
  originals are in. A zoomed image is clipped to the preview's rectangle,
  not its rounded corners.
- The Add Tags and Folders dialogs: the fold
  chevron turns at once rather than over 120 ms, row highlights do not fade
  over 60 ms, and a folder's own glyph is drawn as the plain folder (`TODO`).
- The pickers: with
  several items selected, a picker starts from the tags or folders they all
  share (production starts from the details of the item last shown). The
  tag picker's folded rail and pin last for the session, as in production.
  Not drawn: the create
  row's sticky position and the 60 ms row highlight transitions.
- Settings: only the settings listed under Implemented change the app; the
  others change the settings model and are saved, with a `TODO` where they
  belong: language. Image scaling "Pixelated" draws the viewer's, Quick
  Look's and the detail window's images nearest-neighbour (not a
  collection's members or the navigator, as in production; see
  `vendor/README.md`). Zoom Level is saved and not applied,
  as in production. The grid's
  mouse wheel (scroll or zoom) and the transparency grid are applied.
  Update checks, cloud sync, pause, snapshots, restore points and retention
  writes, and the AI model runtime (download, optimize, delete, which models
  are downloaded) are drawn but not performed; every model shows as not
  downloaded, and only the storage they take is read (`ai.status`). An audio
  tile's tint is the dark theme's in every theme, and the native
  transparency themes (vibrancy, Liquid Glass, Mica, Acrylic) fall back to
  Dark. Selects have no typeahead, the section
  headings' letter spacing is not drawn, the window can still be zoomed and
  made full screen, and a binding field cannot capture keys the app binds
  (⌘Q, ⌘,). An undo in the main window that reverts a settings write does not
  reload the model.
- Subscriptions: no run, source run or gallery download has been started
  from this app yet (they are checked by the commands they send). A run's
  own pop-ups (`subscriptionsSettle.ts`: "Subscription paused", "Query
  completed", "Subscription completed") are not ported; the batch's system
  notification is (see Notifications), and so are the screen's errors and a
  settled gallery download's result. Not built: the accounts window
  (Accounts, a login warning's click and Fix login). The cover's crop is
  drawn from the thumbnail rather than the original. The native `title`
  tooltips of truncated cells are not drawn.
  A cover saved as a rendered crop shows here; production's media protocol
  serves a thumbnail only for a hash that has an original, so it falls back
  to the download glyph for those covers.
- Drawn with their hover states and tooltips but not wired: in
  the library switcher's popover
  choosing another library, Create Library, Open Library and Library Manager
  (those rows only close the popover). Each has a `TODO` where its action
  belongs.
- Filter bar: a change of filters is not a step of Back and Forward as
  `navigateWithGridFilters` makes it (the palette's colour included); the
  grid goes to its top on each change.
  The type menu's rows are the facets known when it opens and refresh when
  new ones arrive. The colour editor's hue strip is six two-stop bands, its
  square is not clipped round, and the hex field neither capitalises as typed nor
  stops at seven characters. The range editors' unit is the shared select,
  not a native one. Menus with editors (the filter bar's) do not reach past
  the window's edge. A select's pop-up list (inside a pop-up menu) has been
  checked by tests of its placement only, not seen: the off-screen tool
  draws menus in the window.
  A layout change keeps the anchored tile without the scope fade production
  runs for it. The view menu's sort row keeps its height while open when
  Random hides the direction pill. The bar, the tag picker in its filter
  mode, the colour editor and the view menu have been seen drawn off-screen;
  the calendar, the range editors and saved filters have not.
- Inspector: a turned stack card is drawn into a bitmap of its own off the
  drawing thread (`inspector/turned.rs`); it lies straight until that is
  ready, and while it enters it grows and fades but keeps its resting turn
  instead of turning from 10 to 5 degrees. The folder card's radial light is
  a vertical gradient. Auto Tag is drawn (dimmed when nothing in it is an
  image) but the AI tagger it opens is a `TODO`. The
  summary spinners and the 250 ms switch to the loading skeleton are not
  ported, and a selection's covers follow the order items were chosen in this
  session, not the Electron query order, for shift ranges.
- The subfolder tiles' cover is a vertical gradient rather than a radial
  one, a cover that cannot be read shows the folder glyph rather than the
  broken-file art, and the rename field's text is not centred. Items
  dragged out of the window show the system's file-type icons rather than
  production's composite ghost, and are only handed to the system on macOS
  and Wayland (`Window::start_platform_drag`); elsewhere the drag ends at
  the window's edge. Escape does not cancel an item drag. The audio tile's
  radial gradient is approximated by a linear one and its `brightness(1.5)`
  is not applied; the hover preview does not apply EXIF orientation.
- The activity
  button's tooltip opens below it, not to its right as `position="right"`
  puts it. Tab from the grid
  toggles the panels as in production, so the rows are entered from the
  tree filter; section headers are not in the keyboard order. In production
  Down in the tree filter does nothing and Tab leaves it for the grid, so
  this row keyboard has no Electron counterpart; whether to keep it is open.
  Production's folder drag ghost is drawn 48px below the pointer (its
  `position: fixed` is measured from the rail below the titlebar); here it
  sits on the pointer as `Sidebar.tsx` means it to. The smart-folder
  editor's notes text area does not scroll past its three lines or show
  the resize grip, the file types are the
  six built-in choices rather than the accepted formats. The icon picker shows the sidebar's
  glyphs (filled, 1.2 stroke) rather than the 1.5-stroke outlines, and its
  tiles light at once rather than over 50 ms. The sidebar's
  preferences (which system rows and sections show, counts, guides, the
  double-click action, Quick Access) come from the settings model and are
  written through it.
- Word and PowerPoint viewing is out of scope: those files import, keep
  their embedded thumbnail and open in the default application.
- Document viewers: production's live PDF text layer (selectable text) is
  not ported, nor is text selection on the text and book pages, nor the
  editable font sample (`contentEditable`). RTF shows its text without
  character formatting (rtf.js renders it). Markdown images and raw HTML
  show as their text; an EPUB chapter's own stylesheet is ignored. Text
  files over 4 MB show their first 4 MB and say so. A page larger than the
  viewport starts at its top-left corner (production's centred grid cuts off
  what overflows above and left); only the vertical scrollbar is drawn. The
  glyph grid does not enlarge the hovered glyph (GPUI cannot scale an
  element). WOFF2 fonts are not unpacked (Picto does not import them). The
  font is registered with the text system on the drawing thread. A PDF
  page is drawn at most 8192 pixels on its longest side. Encrypted PDFs show
  a message. Quick Look scrolls a document's pages with the pointer only
  (its Space closes it). A comic archive is measured from each page's
  header, which reads every page once on opening.
- Flash: the media keys (K, the arrows and M) do not reach Flash's
  controls; the registry's Video shortcuts drive the media player only. The
  stage is 960×720, not fitted to the movie's own size (`fitFlashStage`).
- Seen drawn in the app (off-screen, see Checks): a PDF in the viewer with
  its page footer. Not used with a pointer yet: the other document viewers,
  the font viewer and Flash's controls; hover states, tooltips, scrolling,
  the volume drag and the controls' fade are unverified.
- Media player: Fullscreen (the button, F and a double-click on the
  picture) shows the video's player alone over its window, which goes full
  screen through GPUI's window API (all platforms); Escape, F, the button or
  a double-click go back, as does leaving full screen another way or the
  item closing. A window already full screen stays so. Off-screen pictures
  only: the macOS full-screen transition itself has not been seen. The rate menu is production's
  own small black list (`.rateMenu`), not the shared context menu. The
  frame steps exist as actions without keys, as in production. The volume display fades in but cannot grow
  from 90%. The visualizer's reflection blurs the spectrum's bars (drawn as
  shadows) but draws the oscilloscope and orbit's blur as three faint
  copies; the radial glow is concentric discs. On Windows and Linux video
  goes through the bundled `ffmpeg` and audio through Symphonia and CPAL;
  both are built and unit-tested in CI there but have not played a file
  (CI has no ffmpeg and no sound device). A scrub restarts the ffmpeg
  processes at most every 100 ms. An APNG plays as a still image (GPUI
  decodes PNG as one frame). A video's picture is uploaded per frame where
  `ffmpeg` decodes it; only the AVFoundation path is zero-copy. Seen
  playing in the app: a `.mov` (AVFoundation), a `.webm` (`ffmpeg`) and an
  MP3 with the visualizer, each with its controls. Not checked: timings,
  hover states, the volume and rate popups; what was checked headlessly is
  listed under Checks.
- Collections: in the detail window members and the space between them
  have no menu. The editor's drag does not become a drag of files out of
  the window. After grouping, the new group is not selected. The organizer's radio is
  the app's round check, not the system's.
- Quick Look: GPUI clips to rectangles, so the image area's 8px rounded
  corners do not clip a zoomed image.
  Its counter follows the selection at once, where production's trails until
  the next image has decoded. Stepping scrolls the grid only enough to show
  the item, not to its centre, and closing does not scroll the grid back to
  the item. Space paging the grid jumps where production scrolls smoothly.
- Detail window: its menu's Search by Image does nothing yet (see the
  item menu); Set as Library Cover opens the dialog in the main window
  (production's detail window has no dialog layer, so the row does
  nothing there). No per-image zoom memory or "Loading..." placeholder; the
  pinned button rests at full strength rather than production's 94%. Moving
  the window by its toolbar does not keep the toolbar showing as
  `picto:window-moved` does. Its size uses the primary display's work area,
  as production does, even when the main window is on another display.
  Always on top and the aspect lock are set through AppKit, on macOS only.
- Context menus exist for grid items, empty grid space, the viewer (media view, Quick Look and detail window), the
  inspector's preview, the sidebar, the viewer's zoom percentage,
  subscriptions, and tags and tag groups on the Tags screen. The grid shell,
  the viewer toolbar, the Subscriptions screen and the Tags screen each host
  the menu as a full-window layer; the menu itself chooses between its
  pop-up windows and the in-window panel.
- Item menu rows shown, enabled as production has them, that do nothing yet
  (each with a `TODO` where it is received): Search by Image (production
  uploads the original from a hidden browser window), Auto Tag, Open
  Folder (the grid shows no folder tiles yet). Copy puts the files on the
  macOS pasteboard (`NSFilenamesPboardType`, as production), on the
  Windows clipboard as `CF_HDROP` (as production's native addon), and on
  Linux as production's `text/uri-list` through `wl-copy` or `xclip`, which
  must be installed (GPUI's clipboard holds text and images only); the
  Windows and Linux paths have not been run.
  Production shows no notification when a menu's Copy, Regenerate
  Thumbnail or Set as Thumbnail fails; these show one. "Add Tags", "Add to Folder" and "New Smart
  Folder" dispatch the shortcuts' `AddTags`, `AddToFolder` and
  `NewSmartFolder`, which open the Add Tags and Folders dialogs (the
  inspector's pickers from the viewer's menu) and the smart-folder editor.
  "Open in New Window" opens the item alone in a detail window.
  "Add to “…”" appears once a folder has been used (the folder picker and
  this row record it, and it is kept across launches). Edits that change what the destination
  holds (Remove from Folder, Sort by, Group, Ungroup, the lifecycle rows,
  deletes) fetch the grid again, which clears the selection; after a rating
  or tags pasted on several items the inspector's selection summary is not
  read again.
- The pop-up menu windows have been seen on macOS reaching past the main
  window's edges, a submenu included, and closing on a press in the main
  window; the system's own shadow and edge highlight are turned off on them.
  Not verified: the pointing-hand
  cursor (GPUI sets the cursor only from the key window), and the input
  method's candidate window, which appears at the invisible twin near the
  main window's corner. Clicking a panel makes its window the key window for
  the moment of the click, so the main window may briefly look inactive; the
  keyboard is handed back to it when the menu closes. The panels do not
  follow the main window if it moves while a menu is open.
- The library popover: only its surface scales in;
  its rows appear at full size. A library's folder has no hover title. A
  cover is cropped once per session, so a cover changed elsewhere meanwhile
  shows the old one. A library with an icon but no cover shows the books
  glyph, not its icon.
- No cloud, localization or site login.
- Export: production reports nothing when Export As or the item menu's
  Export Originals finishes, and nothing when Export As fails; here every
  export shows a notification with what it wrote and the first reason a
  file was skipped. Exports show no progress (production shows none either:
  `media.export` and `picto_pack.export` answer once they are done and
  report no progress). The file dialog for Import Picto Pack takes every
  type, and a `.picto-pack` opened from Finder is not received (no
  associated-file handling). The dialogs' system windows have not been used
  with the pointer; the off-screen tool answers them (`answer`).
- Imports: the file chooser
  takes every type (GPUI's prompt has no type filter), and its "Import
  files" title is the macOS prompt button. A drag from a browser is not
  handled: GPUI delivers only file paths, so the in-memory files and links
  production materializes (`picto:drop:materialize`) do not arrive. One
  dropped path is a folder when it is a directory: production takes any one
  path without a media extension for a folder, which sends a single Flash,
  EPUB or audio file to the folder dialog, where it cannot be imported.
  The task dialog fades in over 200 ms but does not grow from 96%, and its
  rows' and buttons' focus rings are not drawn. Text, Markdown, JSON, RTF,
  EPUB, Word and PowerPoint files get no thumbnail of their own (production
  draws one in a hidden window, `documentThumbnailService.mjs`), so they
  show the torn page. A refresh in place reads as many items as were
  loaded; a selected item that new items push past them is no longer
  selected (production keeps an explicit selection by id). Paste Import,
  the file and folder choosers and the empty states' buttons have not been
  used with the pointer (they open system windows); the drop, the folder
  dialog, the collection question, the task dialog and bar have been played
  off-screen and compared with the reference.
- Viewer: the breadcrumb belongs to the collection editor, which does not
  exist yet. No per-image zoom
  memory and no copy shortcut. Next stops at the last loaded item
  until the next page arrives. The very first opening shows one empty frame
  while the pane is measured.
- Shortcuts not performed yet, bound to nothing and listed with their
  reasons in `keymap::NOT_YET`: Add to Last Folder, Auto-Tag, Copy Tags
  and Paste Tags, the layout and tile-label keys,
  Toggle Logs; production's deferred ids
  are not bound either. Space scrolls the grid
  at once rather than smoothly, and `+` also answers to Shift+= (GPUI
  matches the typed character), where production's `+` needs the key
  itself.
- Application menu: rows whose feature is missing are present and disabled:
  Library Manager, New Library, Open Library and Recent Libraries (only the
  open library is listed; opening real libraries awaits a decision), Check
  for Updates, Auto Tag and Log Window. Copy outside a text field copies
  the selection's files, as ⌘C does (production's row, chosen with the
  pointer, copies nothing there). A
  row is enabled by whether its action has a handler where the keyboard is, so the selection's rows are disabled while Settings or a
  detail window is in front, where production's stay enabled and do
  nothing; Set Rating's submenu title stays enabled (AppKit enables a row
  with a submenu), its rows are not. A key the window leaves alone reaches
  the menu, as in production, so ⌘1–⌘4, ⌃R or ⌥⌘C act while a text field
  has the keyboard; the rows on ⌘⌫ and ⇧⌘⌫ ignore it there, leaving the
  field its editing key. Close Window hides the main window rather than
  closing it, so the window keeps its state when it comes back; production
  makes a fresh one. App-wide zoom (production's ⌘= / ⌘− / ⌘0 outside the
  viewer, a CSS `zoom` on the page) is not ported: every size here is in
  pixels, and GPUI scales only `rem` units. The real menu bar has not been
  seen: AppKit adds its own rows (Edit's Dictation and Emoji & Symbols,
  View's or Window's Enter Full Screen, the Window menu's tiling rows), and
  the bold application menu title is the process's name.
- Motion: GPUI cannot scale an element's contents, so where production
  scales a panel (menus, the library popover, dialogs, tooltips) the panel's
  surface, border and shadow are drawn in the scaled rectangle about the
  same origin while the contents stay at full size; menus and dialogs clip
  their contents to that rectangle, tooltips and the library popover do not
  (a dialog clips only once its height has been measured, from its second
  frame). While a rail or dialog fades out it still takes the pointer, where
  production sets `pointer-events: none`. Still to port with the features
  they belong to: a subscription's run progress fading in, the 250 ms
  switch of a slow selection summary to the loading skeleton, smooth paging,
  and the spinner loops.
- Context menus shown as pop-up windows (the default on macOS) are not
  blurred: their window is transparent, so there is nothing of the app's
  behind them to blur, and the system's material is not yet masked to the
  panel (`PICTO_GPUI_MENU=window` shows them blurred, in the window). No
  backdrop blur on Windows (DirectX renderer), and no window vibrancy. GPUI
  cannot scale or rotate arbitrary elements; see Motion above for what stands
  in.
- UI zoom (production's ⌘+, ⌘- and ⌘0, kept in `picto-zoom-factor`) is not
  built, so there is nothing to keep. Settings' Zoom Level is saved but, as
  in production, not applied.
- The main window's place is restored by code that has only been tested in
  its pure parts (`state::WindowPlace::on_displays`); the off-screen tool has
  no real window, so opening at the kept bounds, maximised, on a second
  display, has not been watched.
- Home, End, Page keys and shift-moves in the grid go to the destination's
  first and last items; production's `useGridArrowNav` stops at the ends of
  the loaded window. A search keeps no scroll position (production keeps
  the old offset, clamped). The windowed grid has not been run on a library
  of a million items; its per-frame work and memory are bounded by the
  window, not measured at that size.
- Accessibility exposes the native window but not the control tree.
- The grid is still one element tree per tile.
- No frame-time, decode-latency or memory benchmark has been completed. Do not
  call the prototype faster, flicker-free or memory-bounded on this evidence.
- Windows and Linux are built and unit-tested in CI, and CI photographs the
  app drawn off-screen on both (see Checks); no one has used it on a screen
  there. Seen in those pictures: the grid, the viewer, menus, Settings, the
  app's own window buttons and the in-window application menu. Not
  exercised by them: the systems' window frames, titlebar dragging, resize
  edges, the clipboard, notifications, video and sound. Windows and Linux
  have their own minimise/maximise/close buttons (Windows' as system caption
  buttons), Linux client-side decorations with resize edges, the application
  menu behind a titlebar button (`ApplicationMenuButton`), keys written
  `Ctrl+Shift+S`, and Quit per platform; closing the main window quits
  there (a detail or Settings window does not outlive it, as it can in
  production). Not built there: dragging files out of the window (GPUI has
  a native file drag on macOS and Wayland only; Windows needs an OLE
  `DoDragDrop` run outside GPUI's event dispatch, X11 an XDND source), the
  detail window's aspect lock (Windows `WM_SIZING`, X11 size hints) and,
  on Linux, always-on-top (its pin is hidden there); the UI font is the
  system's (production bundles Geist on Windows and Linux).
  The Ruffle frame path reads back from the GPU and uploads again; it is
  not zero-copy.

## Checks

```sh
cargo test --manifest-path experiments/gpui/Cargo.toml
PICTO_GPUI_TEST_LIBRARY="$(PICTO_GPUI_COPY_ONLY=1 experiments/gpui/run.sh /path/to/Main.library)" \
  cargo test --manifest-path experiments/gpui/Cargo.toml -- --ignored --test-threads=1
```

The ignored test `real_copy_answers_commands_and_reports_changes` opens the
application on the copy, sends commands before and after it has opened,
sees a write's `library/changed` arrive, runs a duplicate scan and keeps one
file of a pair, checking the source library's blob is unchanged.

The regular tests cover the sidebar's drop positions, valid drop targets,
move targets and planned reorders, the smart-folder rule model compiled to
`FilterExpr` and read back (dates, colours, ranges, exclusions), the
deletion messages, the grid-drop rows, keyboard stepping through the rows
and the drag threshold, the Tags screen's sections, rows, ordering, search
ranking, shimmer and starred-tag edits, the dialogs' timing, the context menu's search ranking, separator
cleaning, keyboard stepping and placement and flip arithmetic, the duplicate review's linked zoom, queue order,
labels and difference picture, the three layout modes against hand-computed geometry,
the viewer's fit, focal zoom, pan, glide, resize and navigator arithmetic,
the detail window's size and aspect ratio, which viewer action each key and
click maps to under each Controls setting, the
logarithmic zoom track, the filter model (the expression built from the
bar, counting, labels, types, dates and ranges), the saved view options'
round trip, the scroll anchor, the calendar, the colour conversions, the
pickers' placement below a control, the view menu's heights,
banded visibility against a full scan of 20,000 tiles in every mode, the
rubber band's hit geometry and auto-scroll speed, the hover preview's timing
and fit, the empty-state copy, the broken and font artwork as rendered, view
preference fallbacks, grid labels, popover placement, the known-library
order and name search, the reveal timeline, the easing curve
against the sampled bezier, scaled frames about their origin, the corner a
menu grows from, the menu's, tooltip's and dialog's enter and exit values,
delayed fades, tree ordering and the text-search gates, and the media
player's time format, scrubber and volume arithmetic, skips, frame steps,
speed steps, seek display, the controls' hide timing, the frame-rate
snapping, the analyser's conversions, resampling and pitch-keeping time
stretching. The ignored tests need a disposable copy, a GPU, media files or
an audio device and are run explicitly:

```sh
PICTO_GPUI_TEST_WEBM=/path/to/video.webm PICTO_GPUI_TEST_AUDIO=/path/to/audio.mp3 \
  cargo test --manifest-path experiments/gpui/Cargo.toml -- --ignored \
  real_webm decodes_and_seeks plays_through
PICTO_GPUI_TEST_VIDEO=/path/to/video.mov \
  cargo test --manifest-path experiments/gpui/Cargo.toml --test avfoundation_frames
```

The last is a plain program (`tests/avfoundation_frames.rs`), since
AVFoundation reports to the main thread, which the test harness does not
give a test; it skips without the variable. It checks that frames come as
the buffers GPUI draws, of the item's size, with the BT.601 matrix, at
advancing times, and that a seek and a paused picture arrive.

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

`PICTO_GPUI_POINTER=<file or FIFO>` replays input written to that file
(`move X Y`, `drag X Y` with the button held, `down X Y [clicks]`,
`up X Y [clicks]`, `rdown X Y`, `rup X Y`, `scroll X Y DISTANCE`, in window
points; `key <keystroke>` and `type <text>`; `dragfiles X Y PATH...`, files
from Finder over the window, `drop X Y PATH...`, the same let go there,
which imports them, and `dragexit`, the platform's file-drop events; paths
cannot contain spaces), so hover, pressed, tooltip,
menu, rubber-band, keyboard and navigation states can be checked without
taking the real pointer. GPUI stops painting a window that is fully covered;
`PICTO_GPUI_BACKGROUND=1` opens the window without taking keyboard focus and
keeps it drawing behind other applications. `PICTO_GPUI_MENU=window` draws
menus inside the window (a pop-up menu closes when the app is not the active
one). `PICTO_GPUI_INSPECTOR=<width>` opens with the inspector at a stored
width whatever was kept, and `PICTO_GPUI_SILENT=1` starts every player muted.
`PICTO_GPUI_STATE=<folder>` keeps what the user arranges in that folder
instead of the app's own (see "What is kept across launches"); the off-screen
tool keeps nothing without it, so its pictures never depend on how the app
was left, and two runs with the same folder show a restart.

The whole app can also be drawn with no screen at all. The ignored test
`plays_a_script` (`src/offscreen.rs`) opens the shell on a disposable copy
in a headless window, plays a script of the same input lines plus
`wait <ms>`, `shot <name>` and `window <n>`, and saves a picture per shot:

```sh
PICTO_GPUI_SILENT=1 PICTO_GPUI_MENU=window \
PICTO_GPUI_COPY=/tmp/picto-gpui-library.XXXXXX/Name.library \
PICTO_GPUI_SCRIPT=steps.txt PICTO_GPUI_OUT=shots \
  cargo test --manifest-path experiments/gpui/Cargo.toml -- --ignored plays_a_script
```

On macOS the pictures are drawn with Metal and the system's text; on Linux
and Windows with GPUI's wgpu renderer (which finds a software adapter on a
machine without a GPU: lavapipe on Linux, WARP through Direct3D 12 on
Windows) and its portable text system holding only the bundled fonts. CI
(`.github/workflows/gpui-experiment.yml`) plays `ci/smoke.txt` this way on
Linux and Windows into a synthetic library and uploads the pictures as the
`pictures-linux` and `pictures-windows` artifacts
(`gh run download <run> -n pictures-linux`). They show the app's own drawing
on those systems, not the platform layer: the window chrome they show is the
app's, the system's (its frame, its menus, its pointer) is not in them.

A system dialog cannot be driven off-screen, so `answer <path>` has the next
folder, file or save dialog the app asks for answer with that path without
opening (`answer` alone cancels it); put it before the line that opens the
dialog. Only the off-screen test reads it (`prompt.rs`). The reference's
dialogs are the system's own and cannot be answered from a script.

`invoke <command> <json>` sends an application command (`folders.create`,
`items.set_folder`, …) and prints its answer, in both tools, so one script
sets a copy up as it sets up the reference's.

It redraws the whole window every tick, which hides a view that changes its
state while a frame is drawn and waits for a frame nothing asks for (on
screen it "fixes itself when the mouse moves"). `PICTO_GPUI_LAZY=1`, or a
`lazy` line in the script (`eager` goes back), draws only when a real
window would: after something invalidated it outside a draw, or asked for
the next animation frame (`Window::is_dirty`, a test-support addition to
the vendored GPUI). Run a script both ways and treat any difference as a
fault. Video does not play off-screen (AVFoundation needs the main run
loop), and the tool cannot resize a window.

The reference takes `dragfiles`, `drop` and `dragexit` too, through the
browser's drag events (`Input.dispatchDragEvent`), so a drop imports into
the reference's own copy. Its CSS animations do not run while its window
is hidden; a script that shoots a dialog first adds a style that sets
their durations to 0 through `js`.

`reference.sh /path/to/Name.library [n]` starts the Electron app as an
isolated reference: a development build from this checkout in its
automation mode (`PICTO_E2E`), on its own detached copy of the library, with
its own application data, and a DevTools port (9222 + n).
`node reference.mjs steps.txt outdir` plays the same script lines into it
(`reload` and `size 1213 865` first, to start from a fresh window at that
size; the GPUI test skips those lines), so one script photographs a view
in both apps and `pixel_diff.py` compares the pairs. Both apps then open the
inspector at its default width; the reference has no traffic lights and
shows the default library avatar.

Seen this way on 13 November 2026: the grid and its item menu, Quick Look,
the View options panel, the filter bar with the tag picker and the colour
editor, the Tags, Subscriptions and Duplicates screens, the smart-folder
rules dialog, the Settings window, a detail window, and the viewer on a JPEG,
a GIF, a JPEG XL, a `.mov`, a `.webm`, an MP3 and a PDF collection. Timers
advance with the real clock there, so animations play, but nothing was
measured.

`PICTO_GPUI_FPS=1` shows a frame meter in the window's bottom-left corner:
the frames drawn in the last second, the time between the last two, and how
long the app took to draw one (from the start of the shell's render to the end
of its paint; the GPU's share is not in it). It reads "idle" once nothing has
drawn for a second.

`million.sh [items]` makes a synthetic library (a million items by default:
4,096 generated JPEGs shared among them by hard links, with five smart
folders) with the repository's `million_grid_demo` example and opens the app
on it with the meter on, for checking scrolling, resizing and loading at
scale. It reads no existing library.

`PICTO_GPUI_STATS=1` prints, for each second in which something happened, how
often each view repainted, how long the grid took to build a frame (on the
clock, and in the main thread's own time, with its layout and tile parts),
the gap between grid frames, the longest the main thread kept a 4 ms timer
waiting (a stall), how many thumbnail files were read, let go, let go
without ever being painted, or dropped unfinished. It also prints at once
each stage of opening (database readable, application finished), each
window of results read on the library thread and when it arrived, and each
screen read or edit on the library thread that took 5 ms or more.

Two guards for scale: `a_frame_does_the_same_work_whatever_the_library_size`
(a regular test) checks that the window asked for, the tiles laid out and the
tiles drawn are the same at 3,000, a million and fifty million items; and,
on a real copy of any size,

```sh
PICTO_GPUI_TEST_LIBRARY=<copy> cargo test --offline \
    window_reads_stay_flat_across_the_library -- --ignored --nocapture
PICTO_GPUI_TEST_LIBRARY=<copy> cargo test --offline time_each_stage -- --ignored --nocapture
```

time every read the grid makes (each sort, at the start, middle and end;
smart folders; searches; the summary of everything selected) and fail if a
window deep in the list costs much more than the first, and time each stage
of opening. On the million-item library on 14 November 2026: every window
read 0.3–9 ms, wherever it was; the summary of a million selected items
0.6 ms; opening the database and its resident indexes 1.2–11 s (the
library's own load, varying with the system's file cache); the recovery
passes 13 s alone, 28–30 s with the app drawing beside them.

### Format matrix

`src/viewer/document/matrix.rs` checks every media type Picto accepts
(`core/src/media_processing/formats.rs`) against the renderer it gets, and
that its first page or frame draws: the regular tests decode the fixtures in
`assets/fixtures/documents/` (generated, a few hundred bytes to 2 KB each;
the font is the bundled Fira Mono, also wrapped as WOFF at test time) and
assert each picture's size and that it is not one flat colour. The ignored
tests draw the real viewers through GPUI's headless Metal renderer, off
screen, and assert the window's middle holds a drawing:

```sh
# Every viewer, off-screen; pictures saved when PICTO_GPUI_MATRIX_OUT is set.
PICTO_GPUI_MATRIX_OUT=/tmp/out cargo test --manifest-path experiments/gpui/Cargo.toml \
  viewers_render_off_screen -- --ignored
# One real file (type from its extension, or PICTO_GPUI_MEDIA_MIME).
PICTO_GPUI_MEDIA=/path/to/file.pdf cargo test --manifest-path experiments/gpui/Cargo.toml \
  renders_a_real_file -- --ignored
```

On 13 November 2026 the off-screen run passed for every fixture, and
`renders_a_real_file` passed for copies of five PDFs, the JPEG XL, the text
file, a GIF and a WebP from a real library.

| Type | Renderer | Status | Checked by |
| --- | --- | --- | --- |
| `application/pdf` | PDF (hayro) | renders | fixture page decoded and viewer drawn off-screen; 5 real PDFs |
| `image/vnd.djvu` | DjVu (djvu-rs) | renders | fixture (made with djvu-rs's encoder) decoded and drawn |
| `application/vnd.comicbook+zip` | CBZ pages | renders | fixture decoded (natural page order) and drawn |
| `application/epub+zip` | EPUB chapters | renders | fixture spine, chapter and picture read; viewer drawn |
| `text/plain` | text page | renders | fixture read and drawn; 1 real text file |
| `text/markdown` | text page | renders | fixture parsed and drawn |
| `application/json` | text page | renders | fixture pretty-printed and drawn |
| `application/rtf` | text page (text only) | renders | fixture's text read and drawn |
| `font/ttf`, `font/otf`, `font/collection` | font viewer | renders | Fira Mono loaded, outlines checked, viewer drawn (OTF and TTC untested) |
| `font/woff` | font viewer | renders | Fira Mono wrapped as WOFF, unpacked, drawn |
| `image/jxl` | image camera (jxl-oxide) | renders | fixture (made with zune-jpegxl) and 1 real file decoded; not drawn in a viewer |
| `image/gif` | image | renders | fixture and 1 real GIF decoded by `image` (GPUI's loader) |
| `image/svg+xml` | image (SVG loader) | renders | `assets/fixtures/vector.svg` decoded by resvg |
| `image/webp`, `image/jpeg`, `image/png` | image | renders | 1 real WebP decoded; JPEG and PNG are the library's everyday files |
| `image/apng`, `bmp`, `tiff`, `x-icon`, `qoi`, `vnd-ms.dds`, `x-exr`, `vnd.radiance` | image | decoder linked | no fixture |
| `image/heic`, `heif`, `avif` (and sequences), `x-icns`, `x-tga`, `x-ilbm`, Photoshop, camera RAW, `vnd.dwg`, `x-xcf`, `x-svga` | image | thumbnail only | GPUI's loader has no decoder; the original fails and the thumbnail stays (as in Chromium) |
| `application/x-shockwave-flash` | Flash (Ruffle) | renders | Ruffle's fixture, ignored GPU tests (frames, mouse, Stop) |
| `video/*` (15 types) | media player (`video/`) | built on the video branch | not checked by this matrix |
| `audio/*` (11 types) | media player (`video/`) | built on the video branch | not checked by this matrix |
| Word, PowerPoint | unsupported view | unsupported view | viewer drawn off-screen with a thumbnail |
| everything else (3D, design files, Excel, iWork, HTML, archives, LUTs, …) | unsupported view | unsupported view | routing checked for every accepted type |

## Structure

One model, separate views, shared components:

- `session.rs` is the model: the destination, its results, the selection and
  the viewer. It talks to `backend.rs`, the small adapter to the existing Rust
  owners, and says what kind of thing changed.
- Each part of the window is its own cached view that repaints only for the
  changes it shows: `shell/` (the root, the sidebar, the library popover, the
  toolbar), `grid/` (the grid view with its layout, saved view, labels,
  reveal, keyboard movement, rubber band, hover preview, tile artwork,
  thumbnail cache and originals, and the filter bar with its model and
  editors), `inspector/` (one item, several items, the destination, and the
  tag and folder pickers, which the filter bar opens in their filter mode),
  `viewer/` (the media surface with its zoom and pan arithmetic in
  `camera.rs`, Flash and its controls, the media player in `video/`,
  `document/` (the renderer choice and the document, text, font and
  unsupported viewers, with the format matrix), and its three hosts: the
  media view with the viewer toolbar the titlebar shows, Quick Look and the
  detail window, with `trigger.rs` deciding which a key or click opens) and
  `screens/` (the destinations that are screens of their own).
- `ui/` holds the shared components, one implementation each: icon button,
  tooltip, context menu (with submenus, search and swatches), dialog, popover, the
  floating selection panel of the pickers (`overlay_shell`), library avatar, sidebar row, section
  header, chip, inspector section, action row, action button, property row,
  field frame and text input, scrollbar, "Return to Top", badge, toggle,
  check row, select, number field, slider, window close button and rim, zoom
  control, empty state, the titlebar drag region, the filter bar's field and
  buttons, segmented glyph controls (the match rule and the sort
  direction), the colour editor and the date picker. Views compose these.
- `imports/` starts imports (drops, choosers, paste) through the
  application's queue, asks what an import needs (the folder dialog, the
  collection question), and shows the foreground tasks (the task dialog and
  task bar); `shell/import.rs` is the window's side (the drop and its
  overlay, the menu rows and keys).
- `export/` exports originals, converted files and Picto Packs and imports
  packs through the application's commands, with the Export and Picto Pack
  dialogs; `shell/export.rs` takes the File menu's rows and keys.
  `prompt.rs` opens the system's folder, file and save dialogs.
- `screens/` holds the destinations that are screens rather than a grid.
  A screen can hand the toolbar controls to show in the grid's place.
- `settings/` is the settings model the app reads, the shortcut registry, and
  the Settings window with its sections.
- `theme.rs` holds the design tokens, derived from one background, one
  contrast colour and one text colour as `tokens.css` derives them.
  `motion.rs` holds durations and easing. Views take colours, sizes and
  timings from these and write none of their own.

- `application.rs` runs the application layer (`picto_core`) on the copy:
  the guard that refuses anything but a marked copy, the detaching of
  inherited work, commands and events. The permanent-delete confirmation the
  grid and the sidebar share is `Shell::request_permanent_delete` in
  `shell/item_menu.rs`.

## Attribution

`ui/text_input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Sidebar icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Toolbar glyphs are Picto's own, from `src/shared/ui/icons/toolbar-icons.tsx`.
`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`. The files in
`assets/fixtures/documents/` were made for this project (drawn pages,
encoded with zip, djvu-rs and zune-jpegxl, a hand-written PDF and text).
