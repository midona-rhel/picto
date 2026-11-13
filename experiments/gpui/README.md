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

The generated macOS bundle is `experiments/gpui/target/Picto GPUI.app`.

## How the app talks to the backend

The app runs Picto's application layer, `picto_core` (the `core/` crate),
in-process, as the Electron app runs it through its napi addon:
`application.rs` sets the application data root, registers the event
callback, and opens the copy with `picto_core::state::open_library` on a
Tokio runtime of its own. That starts the same workers as production:
publication, imports and ingest, derivatives (thumbnails, perceptual hashes
and the automatic duplicate scan), full-text indexing, checkpoints, AI
tagging, and the subscription scheduler and runners. `Session::run` keeps
working: the library thread borrows the application's own `Library`
(`LibraryApplication::library()`), so the database is opened once.

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
  the inspector when the shown item is named and drops the saved grids that
  Back would restore; `subscriptions` and `tasks` read whether subscriptions
  run (`Change::Activity`), every 1.5 s while one does. The Duplicates screen
  reads its queue again on `duplicates`, and the Subscriptions screen its list
  on `subscriptions` (after 250 ms of quiet) and `tasks`.
- Still straight to the library (`Session::run` or the backend's requests):
  grid pages and saved views, item details, rating, rename, notes, tags and
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
  1px border), keyboard focus, drop target, drop inside and the 2px
  before/after bars.
- Hover and pressed states from the Electron styles, with their timings: icon
  buttons (hover, pressed, toggled, disabled), sidebar rows and section
  headers (title pill, chevron, add button), the library switcher's highlight,
  chips, the inspector's sections and action rows, the scrollbar thumb, and
  pointer cursors on everything that takes a click.
- The shared context menu as `ContextMenu.tsx` draws it: glass panel at the
  pointer with a 12px margin, icons, shortcut hints, separators, a 30 ms row
  highlight shared by the pointer and the arrow keys (which skip separators
  and disabled rows and stop on submenus), Enter and Escape, and an 80 ms
  fade when dismissed. A press outside closes it and goes no further. Rows
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
- A tile that draws more pixels than its 512px thumbnail holds shows its
  original instead, by the rule of `thumbnailPlan.ts`: still images only,
  never GIF or JPEG XL, after a quarter of a second in the viewport, decoded
  off the drawing thread to exactly the size the tile shows, two at a time,
  and let go when the tile leaves the viewport.
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
- Inspector with several items selected, from the library's selection
  summary: the stacked preview of the last six covers chosen with its
  shade, the turned cards' scale and the enter and exit motion, "N items
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
  once decoded, as `ImageCrossfadeFrame` swaps them.
- A collection's members step with the bottom strip of
  `QuickLookNavigation` (48×32 buttons, monospace counter).
- Space, Enter or a double-click opens and closes the viewer, Escape closes
  it. Originals decode off the drawing thread; stale loads are fenced.
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
  sticky header, hover rows and row buttons. Names rename in place, from the
  card's menu or a double-click on the rail's name, through the library's
  published write. The reads are the application catalog's own, run through
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
- Delete Permanently in the Trash's grid menu and Empty Trash in the
  sidebar, as `permanentDeleteRequest.ts` asks: the selection's media count
  against `undoDeleteLimit` says whether it can be undone, the confirmation
  says which, `items.delete` deletes, and a delete that can no longer be
  undone asks again. Files leave the copy only.
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
  title bar with its close button, body and footer, the 150 ms fade and rise
  in and 120 ms fade out, Escape and a click on the backdrop to close. The
  tag editor, merge picker, group dialogs and confirmations use it, with the
  shared action button (`actionButton.module.css`) and dialog fields.
- Context menus have a row of colour swatches (`ColorPicker`), used by the
  Tags screen's group menu, in both presentations.
- SVG originals through GPUI's SVG loader. Flash through native Ruffle 0.5.0
  (wgpu offscreen rendering, mouse and keyboard forwarding, optional CPAL audio).

## Not yet equivalent to Picto

- Tags screen: ⌘F does not move to its search; a failed save of the tag
  preferences is held as the settings model's error, not shown as a toast; the editor's
  name field is dimmed while an edit runs but still takes typing; Undo does
  not reach tag edits; tag and group changes made elsewhere show when the
  screen is opened again, not live.
- Duplicates: errors, the scan's result ("Found N new review pairs") and
  "Smart merge needs a choice" are not shown, as there are no toasts yet (an
  ambiguous Smart merge leaves the pair on screen); a failed read shows only
  in the empty state. The progress bar's position over a shown pair is a
  2px row above the cards (the Electron `.scanProgress` class has no rule).
  The Smart merge ring follows the pointer only, not keyboard focus. No context menu on the
  candidates (Open with Default App, Open in New Window). The difference
  picture is computed from the decoded images (GPUI has no blend modes), at
  most 4096 pixels on its longest side, from the thumbnails until both
  originals are in. A zoomed image is clipped to the preview's rectangle,
  not its rounded corners.
- Settings: only the settings listed under Implemented change the app; the
  others change the settings model and are saved, with a `TODO` where they
  belong (theme, zoom, language, the grid's mouse and Space actions, preview,
  notifications, imports, rejecting, subscriptions).
  The keyboard preset, language, audio visualization and shortcut bindings
  live for the session only, since production keeps them in the renderer's
  localStorage; the app's own key bindings do not follow edited shortcuts.
  Update checks, cloud sync, pause, snapshots, restore points and retention
  writes, and the AI model runtime (download, optimize, delete, which models
  are downloaded and their storage) are drawn but not performed; every model
  shows as not downloaded. Selects open by pointer only (no arrow keys or
  typeahead), there is no keyboard focus ring or tab order, the section
  headings' letter spacing is not drawn, the window can still be zoomed and
  made full screen, and a binding field cannot capture keys the app binds
  (⌘Q, ⌘,). An undo in the main window that reverts a settings write does not
  reload the model.
- Subscriptions: no run has been started from this app yet (see the test
  card below); failures and completions are not announced (no toasts or
  system notifications). Creating subscriptions, galleries and sources, and
  the per-source Run, Hold, Group, Edit and Delete, are drawn but not
  performed. Also not built: the schedule submenu, saving the schedule select's choice and the posts-per-run field, the cover
  dialog, reset and delete confirmations, adding or removing destination
  folders and tags, the source details and edit windows, the accounts
  window and login warnings, the gallery progress total, and acknowledging
  issues when a subscription is opened. Site names and example queries are
  copied from the source catalog in `core`. Letter spacing on badges and
  table headers, and the native `title` tooltips of truncated cells, are
  not drawn.
- Drawn with their hover states and tooltips but not wired: View
  options, the inspector's Add to Folder,
  Add Tags, Export (for an item, a selection and a destination),
  source-address list and Pin, and in the library switcher's popover
  choosing another library, Create Library, Open Library and Library Manager
  (those rows only close the popover). Each has a `TODO` where its action
  belongs. The Filter button only clears a filter the inspector set; there
  is no filter bar.
- Inspector: the stacked preview's cards cannot turn (GPUI rotates only
  SVGs), so they lie straight; the folder card's radial light is a vertical
  gradient. Overwriting the notes of a selection that already has notes needs
  the confirmation dialog, so it is refused and the field reverts. Auto Tag,
  the subfolder scope, the summary spinners and the 250 ms switch to the
  loading skeleton are not ported, and a selection's covers follow the order
  items were chosen in this session, not the Electron query-selection order
  for ranges and Select All.
- Not in the grid yet: dragging items out or to reorder, middle-click,
  inline rename, the empty grid's context menu, and the subfolder tiles a
  folder shows above its items (an empty folder with subfolders shows the
  empty state). The Import buttons are drawn with a `TODO`. The audio tile's
  radial gradient is approximated by a linear one and its `brightness(1.5)`
  is not applied; the hover preview does not apply EXIF orientation.
- Not in the sidebar yet: drag reorder and
  drops from the grid (the rows have the drop looks; nothing drives them),
  Quick Access, keyboard movement through the rows (the focused look is
  there; nothing moves focus to a row), the New Folder and New Subfolder
  shortcuts, and the confirmation before deleting a folder (it deletes at
  once). Menu rows shown but not performed yet, each
  with a `TODO`: Add to Quick Access, Move to..., Set Auto Tags..., Import
  Folder (Here)..., Attach Watched Folder..., Edit Smart Folder..., Edit
  Rules..., New Smart Folder and New Child Smart Folder (no smart-folder
  editor), Change Icon, Sort Folders, Sort by and Export (submenus). The
  visibility rows close the menu when chosen; the
  colour picker row and Remove Watched Folder are not drawn. The activity
  button's tooltip opens below it, not to its right as `position="right"`
  puts it. Folded nodes and sections are kept in memory, not persisted.
  The sidebar's preferences (which system rows and sections show, counts,
  guides, the double-click action) come from the settings model; its
  visibility menu writes through it to the library at once, as production
  patches the settings.
- No video or audio playback, PDF, text, EPUB, CBZ, DjVu, JPEG XL or font
  viewers yet. Word and PowerPoint viewing is out of scope: those files import,
  keep their embedded thumbnail and open in the default application.
- Collection members use previous/next controls, not the continuous collection
  scroll surface. Preview is embedded, not the three production shells.
- Context menus exist for grid items, the sidebar, the viewer's zoom
  percentage, subscriptions, and tags and tag groups on the Tags screen. The
  grid shell, the viewer toolbar, the Subscriptions screen and the Tags
  screen each host the menu as a full-window layer; the menu itself chooses
  between its pop-up windows and the in-window panel. Subscriptions' Schedule
  row stays disabled rather than a submenu; `subscriptions.schedule`, which
  also sets the next run time, is now reachable and is its `TODO`. "Set
  Rating" is offered for one item only (the library adapter rates one item
  per request), and "More" is left out: nothing this
  prototype can do belongs in it. Custom rows, right-click actions on rows
  (`contextAction`), nested submenus and the open scale-in are not ported.
- The pop-up menu windows have not been seen running. Not verified: that
  AppKit draws no titlebar buttons on them, what the system window shadow
  adds around the transparent window and the drawn shadow, the pointing-hand
  cursor (GPUI sets the cursor only from the key window), and the input
  method's candidate window, which appears at the invisible twin near the
  main window's corner. Clicking a panel makes its window the key window for
  the moment of the click, so the main window may briefly look inactive; the
  keyboard is handed back to it when the menu closes. The panels do not
  follow the main window if it moves while a menu is open.
- The library popover's glass has no blur, and only its surface scales in;
  its rows appear at full size. A library's folder has no hover title. A
  cover is cropped once per session, so a cover changed elsewhere meanwhile
  shows the old one. A library with an icon but no cover shows the books
  glyph, not its icon.
- No import/export, cloud, localization or site login. The grid shown is
  not read again when background work (an import, a subscription run)
  changes it; going to another destination and back does.
- Viewer: the breadcrumb and "Edit group" belong to the collection editor,
  which does not exist yet, so neither is ever shown; collections step
  through members with the bottom strip instead of the reader surface. No
  transparency checkerboard (off by default in Picto), no per-image zoom
  memory, no viewer context menu, rating or copy shortcuts, and no video
  shortcuts (there is no video playback). Next stops at the last loaded item
  until the next page arrives. The very first opening shows one empty frame
  while the pane is measured. When the viewer closes by going to another
  destination, the pane shows the fading grid rather than the fading viewer.
- Motion still to port: the dialogs' scale (they fade and rise but cannot
  scale), the inspector's fade when entering
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
- The grid is still one element tree per tile. The media surface still
  carries colours of its own.
- No frame-time, decode-latency or memory benchmark has been completed. Do not
  call the prototype faster, flicker-free or memory-bounded on this evidence.
- Windows and Linux are not built or tested. The Ruffle frame path reads back
  from the GPU and uploads again; it is not zero-copy.

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

The regular tests cover the Tags screen's sections, rows, ordering, search
ranking, shimmer and starred-tag edits, the dialogs' timing, the context menu's search ranking, separator
cleaning, keyboard stepping and placement and flip arithmetic, the duplicate review's linked zoom, queue order,
labels and difference picture, the three layout modes against hand-computed geometry,
the viewer's fit, focal zoom, pan, glide and navigator arithmetic, the
logarithmic zoom track,
banded visibility against a full scan of 20,000 tiles in every mode, the
rubber band's hit geometry and auto-scroll speed, the hover preview's timing
and fit, the empty-state copy, the broken and font artwork as rendered, view
preference fallbacks, grid labels, popover placement, the known-library
order and name search, the reveal timeline, the easing curve
against the sampled bezier, tree ordering and the text-search gates. The
ignored tests need a disposable copy or a GPU and are run explicitly.

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
file (`move X Y`, `drag X Y` with the button held, `down X Y [clicks]`,
`up X Y [clicks]`, `rdown X Y`, `rup X Y`, `scroll X Y DISTANCE`, in window
points), so hover, pressed, tooltip, menu, rubber-band and navigation states
can be checked without taking the real pointer. GPUI stops painting a window
that is fully covered, so the window has to be visible.
`PICTO_GPUI_INSPECTOR=<width>` opens with the inspector at a stored width, and
`PICTO_GPUI_BACKGROUND=1` opens without taking keyboard focus.

`PICTO_GPUI_STATS=1` prints, for each second in which something happened, how
often each view repainted, how long the grid took to build a frame, the gap
between grid frames, and how many thumbnail files were read.

## Structure

One model, separate views, shared components:

- `session.rs` is the model: the destination, its results, the selection and
  the viewer. It talks to `backend.rs`, the small adapter to the existing Rust
  owners, and says what kind of thing changed.
- Each part of the window is its own cached view that repaints only for the
  changes it shows: `shell/` (the root, the sidebar, the library popover, the
  toolbar), `grid/` (the grid view with its layout, saved view, labels,
  reveal, keyboard movement, rubber band, hover preview, tile artwork,
  thumbnail cache and originals), `inspector/` (one item, several items, and
  the destination), `viewer/` (the media surface with its zoom and pan
  arithmetic in `camera.rs`, Flash, and the viewer toolbar the titlebar
  shows) and `screens/` (the destinations that are screens of their own).
- `ui/` holds the shared components, one implementation each: icon button,
  tooltip, context menu (with submenus, search and swatches), dialog, popover, library avatar, sidebar row, section
  header, chip, inspector section, action row, action button, property row,
  field frame and text input, scrollbar, "Return to Top", badge, toggle,
  check row, select, number field, slider, window close button and rim, zoom
  control, empty state and the titlebar drag region. Views compose these.
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
  inherited work, commands and events. `delete.rs` is the permanent-delete
  confirmation the grid and the sidebar share.

## Attribution

`ui/text_input.rs` adapts GPUI 0.2.2's official `examples/input.rs`, Apache-2.0; see
`LICENSE-gpui`. Sidebar icons are Tabler Icons, MIT; see `assets/LICENSE-tabler`.
Toolbar glyphs are Picto's own, from `src/shared/ui/icons/toolbar-icons.tsx`.
`assets/fixtures/interactive.swf` comes from Ruffle's
`tests/tests/swfs/avm1/button_goto/test.swf` at the pinned Ruffle revision in
Cargo.toml; see `assets/LICENSE-Ruffle.md`.
