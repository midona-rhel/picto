# Picto Architecture

Picto is one program, written in Rust. It has three layers, and the app runs all of them in its
own process:

- The app (`app/`) draws the windows with GPUI and owns everything that reaches the rest of the
  desktop.
- The application core (`core/`) owns the commands and the background work.
- The library crate (`picto-library/`) owns the database, the queries and the writes. The crate
  `picto-sources/` holds the subscription sources that the core runs.

Every change to a library takes this path:

`command -> library write -> SQLite transaction -> projection update -> change event`

SQLite is authoritative. Roaring bitmaps make the scopes of root items fast to read, and they can
be rebuilt from SQLite. Every committed write settles its projections, increments the library's
revision, and announces the categories of what it changed. The app reads those categories again.
It does not predict the library's state, and it does not adjust counts itself.

A root is what the grid shows as one tile: a standalone media item, or a collection of media items
in an order. A stored file, a media item and a root each have an id of their own. The members of a
collection have the lifecycle and the folders of the collection. `All` shows only the roots whose
lifecycle is Active; Inbox and Trash show the roots in those two lifecycles.

One file is exactly one media item, enforced by a unique index on `media_item(file_id)`. Every
import resolves a file it already holds to that item: a standalone import adds its tags and
provenance to it, a loose item joins a new collection that contains it, and an item already in
another collection causes the overlapping sets to combine. When several collections overlap one
after another, they all combine into one, which keeps every distinct media item and gets the
tags, folders, notes and source links of all of them and the highest rating. Resolving a duplicate
by keeping one file merges the other item into the kept one (tags, folder places, the higher
rating, the higher-ranked lifecycle of Active, Inbox and Trash, notes and source links), and the
resolution can be undone. A merged-away item's stable
key stays in `library_item_alias`, so re-imports and Picto Packs resolve to the item that
stays. Libraries from before the rule merge their shared files once at open
(`Library::merge_shared_files`), after the schema migration's backup.

Manual imports, watched folders, subscriptions, and retries enter one durable ingest path. See
[Subscriptions](#subscriptions) for how source posts become standalone items or collections.

## Application boundaries

- The app owns the windows, the menus, the views, and the state of an interaction that is not
  stored, such as the selection, the scroll position and an open dialog. It also owns what reaches
  the rest of the desktop: opened files, the clipboard, drags, notifications, the sign-in window
  for site logins, the updater and the packages. The app never writes SQLite itself. Every edit
  is a call to the library or a command to the core, and the app learns that a change was
  committed from the answer and from the change event.
  [`app/README.md`](../app/README.md), section 4, describes the app's parts.
- The core answers commands by name with JSON arguments (`core/src/ipc.rs`). The app sends them
  through `Application::dispatch` (`app/src/application/`). The app reads the grid's items from
  the library with the query functions of `picto-library`, on one library thread
  (`app/src/backend.rs`). The library is never read on the thread that draws.
- The core and the library own the library sessions, the queries, the writes, the background work
  and the change events. A library is closed before another library can answer commands.
- Work that runs for a long time is stored in the library and can be queried. The progress that
  the app shows is read from that stored work. The app keeps no queue of its own.

## Prepared library views

`items.window` answers a request for a window of a view, given as a position in the order or as a
scroll fraction between 0 and 1. It reads one captured projection snapshot and never opens a
SQLite read: the matching, the ordering, the totals, the summaries and the previews all come from
the in-memory library model described below. The answer holds the window, the position at which
it starts, the first four results of the query, exact counts and one revision. Code that reads a
view page by page from the start keeps using `items.query` cursors; grid navigation and refreshes
do not fetch a separate preview page.

A window request is cancelled through one cancellation token. The text, tree and numeric ordering
in Rust, the building of the summaries and the JSON serialization check it, and the token is
passed explicitly to the parallel text workers. Which roots match and the totals are cached
separately, so a change of the byte totals does not invalidate the cached text matches.

Text filters and text-based smart folders share one matching contract: the trimmed, lowercased
literal of at least three characters is a byte substring of the requested lowercased fields.
Selections of up to 4,096 roots look each root's settled text up directly; larger selections
scan the resident text shards, in parallel above 32 shards. A shard scan runs one vectorized
substring search (`memchr::memmem`) over the shard's text and maps each hit to its row and field,
resuming at the next row in scope; shards with no root in scope are skipped, and a shard with
under 3% of its rows in scope looks those rows up instead. A query that contains a recent
selective query (typing one more letter) searches only that query's matches. FTS settlement takes
the oldest 256 dirty roots per batch through an index, so its cost does not grow with the
backlog. There is no limit on how many roots a search may scan, no
trigram pre-selection of candidates and no fallback to SQL; a common term over a million items is
an ordinary scan of the in-memory text, not a hidden slower path. Every byte of the settled text is ordinary, including an
embedded NUL: text after it stays searchable and a query containing one matches the fields that
contain that byte sequence, while alphabetical order keeps SQLite's NOCASE rule of stopping at a
NUL. The `root_fts` table remains the source of the settled text, and the in-memory matcher is
tested against a full-table `instr()` scan of it, which gives the exact expected result.

A window sorted by the captured or the imported date is read from bit-sliced indexes of the dates,
which hold one bitmap for each bit of the value and keep its sign, so a date before 1970 sorts
before 1970 instead of being counted as 1970. A root without a captured date sorts as the date
`-1`, together with the roots whose date is -1 ms, and among equal dates the root id decides the
order in both directions. A date filter matches only dates of 0 or later, and never a root that has
no such date. The order of a folder-tree scope, newest imported first, is read from the same
indexes. Each of these reads finds only the roots at the positions that the window asks for. The
pages of `items.query` read the same orders in memory: a page sorted by date, size or recently
viewed finds where it starts in one cached complete order, and pages sorted by name, rating,
random order or folder order keep their own pagers. The version of the checkpoint tells the
signed date keys from the older keys, which counted dates before 1970 as 1970, and a checkpoint
with another version is built again from the SQLite tables.

The app's grid holds at most 1,500 items of a view at a time (`app/src/grid/window.rs`), so a
selection that reaches beyond the loaded items is never built from the loaded items. Select All
is a `query` target with exclusions. A Shift-click range is a `range` target: spans between two
roots with their positions in the view, plus the roots added outside the spans and the roots
toggled off inside them (`app/src/grid/selection.rs`). The library reads a span from the resident
order. It trusts the span's positions only while both ends still hold the span's roots.
Otherwise it finds the two roots in the complete order.

### The resident library model

One projection snapshot holds everything a grid read needs: lifecycle, rating, tag, folder,
kind, MIME and color bitmaps; numeric and date indexes; collection and folder orders; media
ownership; cover palettes; the settled search text; the alphabetical name tree; drawing facts
for every root; recently viewed roots in display order; and folder parents for tree scopes.
Every folder has an order and a member bitmap, also a folder without members. An open gives them
to a folder that never had members, which has no rows in `ordered_membership_chunk`, as the write
that creates a folder does.
Drawing facts live in immutable 1,024-root shards that intern each shard's content hashes and
MIME strings; names come from the alphabetical index rather than a second copy. A window
builds its summaries from these records and the bitmaps alone. Detail requests, media
records and duplicate inspection still read canonical tables, the last inside a consistent read
that captures the snapshot it will combine with.

Opening a library rebuilds the text, name, drawing and navigation state from canonical rows in
streaming scans, then reuses the bitmap-and-index checkpoint when it is valid for the exact
database revision or rebuilds that too. The open runs under a scoped cancellation token with no
deadline: a later open, a close or process shutdown cancels it, the migration's statements are
interrupted through SQLite's progress handler, and the resident scans stop at their existing
check points. A cancelled open commits nothing, removes the backup its migration attempt took,
and reports `library open cancelled`; the core reports the stage that is running (migrating,
loading, starting) through `library.open_status`, so that a loading screen can name it. Nothing
else is stored as derived state: there are no drawing pages, text chunks or name nodes on disk,
no format versions, generations or repair passes for each kind of state, and no helper reader
connections. The migration from the supported earlier schema backs up the database, drops those
tables and defines the journal of changed roots again. Every open builds the in-memory state,
and the startup statistics report the time of each stage.

SQLite triggers on roots, cover media and cover files journal affected root IDs in
`browsing_dirty_root`. Every publishing transaction settles that journal before commit and in
the same transaction as the canonical change: the affected drawing shards are rebuilt from
canonical rows, the affected names are refreshed, and the recently viewed or folder-parent
lists are reloaded when the receipt names them. Imports, edits, tags, ratings, grouping, cover
changes, deletions, repairs and history replay all publish through this one owner; a failed
transaction publishes nothing. Maintenance writes publish no snapshot and are rejected if they
leave the journal dirty. Shards, tree nodes and bitmaps are shared between consecutive snapshots,
so readers that captured an older snapshot finish on it and a mutation copies only what changed.
Session history retains canonical records for affected roots, never library-wide snapshots.

The memory that the in-memory model uses is measured, not limited. The snapshot's own estimate
counts each shared allocation once for every snapshot that it is part of, and the memory probe
reports the older snapshots that readers still hold separately from the current model.

### Structural history ownership

Collection undo entries retain only the affected canonical root records, member notes,
lifecycle/rating/tag memberships, collection order, and anchored folder positions. They do not
retain library-wide browsing snapshots. Replay checks the affected current state, restores it
inside the canonical transaction, and settles only those roots in the current projections.
Unrelated imports, memberships, derived media facts, and folder members remain current; replay
does not replace them with an older library snapshot. Folder positions are merged in one pass,
preserving the relative order of unaffected members and additions made after the original edit.

Smart-folder results are stored per folder and settled incrementally. A write names what it
changed (a tag, ratings, root text, cover facts, folders, lifecycle); only folders whose rules can
observe that change are re-evaluated, and only over the roots the write touched, with every clause
intersecting that set before it reads a full index. A folder's result and its cache version change
only when its membership actually changed. `smart_settle_oracle` checks this against a fresh
evaluation of every folder after each step of a random edit sequence.
Results cover roots in every lifecycle; a smart-folder scope and its sidebar count intersect them
with Active on read, as every other scope does. Trashing, restoring or accepting Inbox items
therefore re-checks no smart folder, however many exist.

Every SQLite connection sets its own page cache (64 MB for the writer, 16 MB per reader) instead
of relying on the operating system's file cache, which differs widely between Linux, Windows and
macOS; memory-mapped I/O is off. Indexes that only make queries faster are in
`accelerator_indexes.sql` and are created when a library is created and when it is opened;
adding one needs no fingerprint migration. SQLite's automatic checkpoint is off: the runtime
checkpoints the WAL into the database every minute and truncates it on close.

### Writes, history and background work

Every canonical write goes through `Library::publish` (in `picto-library/src/mutation/`, one file
for each kind of thing written: tags, folders, smart folders, imports, duplicates, structure,
undo). It opens the
writer transaction, settles the delta, publishes the snapshot and records any history entry.

Persistence follows the change, not the size of the thing changed. Bitmaps are stored in 64k-id
shards and a write re-encodes only shards holding ids that changed in that bitmap (grouping
ten roots rewrites each tag's one or two shards, not every shard the selection spans). An edit
leaves bitmaps that hold none of its roots shared with the previous snapshot rather than
copying them. Folder and collection orders are
stored as 1,024-id chunks (`ordered_membership_chunk`); an append rewrites one chunk. A history
entry for a bitmap keeps only the members it changed, and replay checks and restores only those
members: later edits to other roots never block an undo, while a later edit to the same roots
does. Replay re-checks only the smart folders the replayed change can affect.

A permanent delete (emptying Trash, or rejecting with the "Rejected items: Delete permanently"
setting) is undoable when it holds at most `undoDeleteLimit` media items (default 500), counting
collection members; zero disables delete undo. Its history entry
keeps the deleted roots' structure and every row that pointed at them, and its files stay on disk
while the entry is in history: `protected_cleanup_files` keeps them out of the blob cleanup, which
removes them once the entry leaves history, at the latest when the app closes. A larger delete
records no entry and queues its files for removal at once; the app asks the user to confirm a reject that
cannot be undone, and it counts the items with the selection that the library resolved, not with
the items the grid has loaded. History protection is registered
before releasing the writer, and cleanup reads it after acquiring the same writer. Undo restores
recent views and cascaded queue jobs, assigning new queue IDs and requeuing interrupted running jobs.
When the app has told the user that a delete can be undone, it passes `require_undoable`; the
serialized writer checks the resolved selection against the limit and refuses the delete if it
no longer fits. The delete is then retried without `require_undoable` only after the user has
confirmed again that it cannot be undone. A delete request waits for the confirmation and for the
write, and reports success only after the write commits; when it is cancelled or fails, Inbox
review does not move to the next item.

A receipt names only what the write changed. Item ids are sent only with the resource `library`,
and a receipt of bookkeeping (tasks, subscriptions, settings) never makes the change event of a
frame report a change of the whole library. `smart_folders` is named only when the result or the
rules of a smart folder changed. The values that the media work computes are announced as
narrowly: a perceptual hash announces nothing, and a palette refreshes only the colour index. The
core caches its answers to `items.query` and `items.mime_facets` by the query epoch, which
bookkeeping does not change.

Background workers (ingest, FTS, media work, AI tagging, subscriptions) sleep until the library
publishes a change, with a fallback probe every 2-30 s for work that becomes ready with time alone
(`core/src/work_wake.rs`). An idle library starts no writer transactions between probes. Imports
publish prepared items in groups of up to 48, flushed at least every 250 ms so the grid fills
progressively. Auto-tagging on import writes in batches outside the undo history; manual AI tagging
stays undoable.

Manual imports are foreground tasks (`task` table, `picto-library/src/mutation/tasks.rs`,
`core/src/import_tasks.rs`). Starting one only records the task. The scan worker walks its paths
and queues one `ingest_job` per file (`task_id`, `units`, `prepared = 0`) in batches, without
reading any file, so a large folder is recorded at once and survives a quit; an interrupted scan
runs again from the start and keeps the jobs it already queued (keys are per task and path). The
ingest worker reads, hashes and probes a few files ahead of each ingest batch, then imports them as
usual; claims skip unread files (`idx_ingest_job_claim`) and the jobs of a task that is not
running. Pause holds a task's jobs in the queue across restarts; Stop drops the ones not yet
imported. Stop-and-undo removes the roots the task created (`task_created_root`) and the tags it
added to roots that already existed (`task_added_tag`); roots it reused stay. An import that moves
its files into the library cannot be undone. `tasks.list` reports each task with a rate and an
estimate measured over the last 20 s, and the app shows it in the foreground task dialog or,
once the task was sent to the background, in the task bar.

A merge of another library into the open one is such a task (`core/src/library_merge.rs`). Its
stored input names the other library's folder, and `tasks.list` reports it with the kind `merge`.
`library.merge.start` checks the other library, writes a backup of the open library's database
(`VACUUM INTO library.sqlite.pre-merge-<milliseconds>.backup`, the migration's
`schema::backup`) and only then records the task. The scan opens the other library's database
read-only with SQLite's `immutable` flag, holding that library's lock file while the database is
open, and reads it with `picto_library::content`: the tags, folders and smart folders once, and
the items in one pass in which the tag bitmaps and folder orders are turned round to tags and
folders per item. It creates the tags, folders (matched by name under the same parent) and smart
folders that the open library lacks, and queues one prepared job per item whose file path is the
file in the other library's blob store. The ingest worker copies each file, verifying its hash,
and the thumbnail beside it, and imports the items through `ingest_merged_batch_for_task` and
`ingest_merged_collection_for_task`, which run the import of any other task. A job of a merge
carries two things that an ordinary import does not have (`MergedImport`,
`MergedCollectionImport`): the time of the item's last change in the other library, which a new
item gets in place of the time of its import, and a collection's own note, which a new collection
gets in place of the note of its cover file. An item is recognised as one the library holds only
by the hash of its file, because the keys of two libraries are not comparable. For such an item a
merge gives `ingest::Held::Merge`: the item keeps its name, dates and lifecycle and gets the
tags, folder places and source links of the other item, and its notes and rating where it has
none. The task records what it created and changed (`TaskUndo`): roots, added tags, folder
places and ratings of existing roots, folders, smart folders, tags and tag namespaces. A merge
either brings everything or is undone: `library_merge::settle`, which the ingest worker calls,
marks a stopped merge and a merge with a failed file for undo, and removes the backup once the
merge has finished or has been undone. The other library's subscriptions, watched folders,
settings, recent views and duplicate decisions are not read.

`picto-library/examples/library_ops_benchmark.rs` measures these operations on a generated library
(`-- [roots] [runs] [smart_folders]`, by default 1M roots and 100 smart folders). With `--sync` it
measures them with sync on, and also measures applying change records from another computer,
among them a tag on up to 200,000 items with and without writes of the user at the same time. It
also measures turning sync on with a cloud folder in a temporary directory, which writes the first
snapshot, syncs through that folder, two writes with and without the sync running beside them, and
opening the cloud library in a new library. Its output is local and never committed.

### Recording changes for sync

Cloud sync (`picto-library/src/sync/`) describes the library as entities with sync keys that are
the same on every computer: an item is keyed by the content hash of its file, a collection, a
folder and a smart folder by their stable UUID, a tag by its full name and a tag group by its name.
The library's settings are one entity with the empty key, the settings of a view are keyed by the
view's scope with the local id in it replaced by the folder's or smart folder's UUID
(`folder:<uuid>`, `smart:<uuid>`, `grid:defaults`), and a "Not duplicate" decision is keyed by the
content hashes of its two items, sorted and joined with `:`. Each entity has fields (an item's
name, rating, sources, the time it was last viewed, `tag:<name>` and `folder:<uuid>` with a
position key, a folder's `place`, a smart folder's rules with tag names and folder UUIDs in place
of their ids, one field `setting:<name>` for each field of the `application` setting except
those that stay on each computer, and so on), and a change to a field is a change record with a
hybrid logical clock timestamp. The fields of the settings that stay on each computer are listed in
`LOCAL_SETTINGS` in `picto-library/src/sync/local.rs`: the window zoom (`zoomFactor`), the window
background effects (`frostedGlassRails`, `acrylicRails`), trackpad gestures
(`viewerTrackpadGestures`), automatic import of watched folders (`autoImportEnabled`), the
switches of AI tagging (`aiTaggerWd14Enabled`, `aiTaggerE621Enabled`, `aiTaggerEva02Enabled`,
`aiTaggerOppaiOracleEnabled`, `aiTaggerAutoOnImport`, `aiTaggerWriteRating`) and the Subscriptions
entry of the sidebar (`showSidebarSubscriptions`), and the subscription fields, whose names start
with `subscription`. The starred tags and the colours of tag groups are
fields of the settings and name tags and tag groups by name. The records are in the sync file,
`sync.sqlite` next to the library database, which exists while sync is on
(`Library::start_sync_recording`, `stop_sync_recording`). `sync_value` keeps, for each field, the
record with the latest timestamp; the library is what those values show. `sync_identity` gives
every key a number, which `sync_value` stores in place of the key, and remembers the local id each
key had, so that a deleted or renamed entity is recorded under its old key. `sync_outbox` keeps
this computer's records until a change file holds them. The sync file has its own connection, which
attaches the library database to read it; library writes never write the sync file and never wait
for it. The connection is opened for each use and closed after it. The bundled SQLite keeps the
cached pages of all connections of the process in one pool, and a connection whose cache is full
reuses the least recently used pages of any connection, so a sync connection that stayed open took
the cached pages of the library's writer on every pass, and each library write read them from the
file again.

A library write does no sync work apart from one row in `changed_entity` for each item,
collection, tag, tag group, folder or smart folder whose own row it changes and for each item whose
sources change, one row in `changed_row` for each change to the `application` setting, a row of
`view_pref` or a duplicate pair that has or had the status Not duplicate, written by triggers that
exist while sync is on, and the index on the revision of `canonical_bitmap` rows that sync adds.
Viewing an item writes `recent_view` with no trigger. The recorder (`Library::record_sync_changes`,
which the core's sync worker calls about every second) reads the library in a read transaction of
the sync file's connection and finds what changed after the revision it last recorded: the local
ids in `changed_entity` and the rows in `changed_row`, the `canonical_bitmap` rows with a later
revision (a shard that becomes empty stays as an empty row, so that removals have a revision too),
compared with its copies in `recorded_bitmap` to get the roots that entered or left each lifecycle,
rating and tag, and the `ordered_membership_chunk` rows whose checksum differs from its copies in
`recorded_order`. For Recently viewed it keeps a copy of the list in `recorded_view`, with the
latest view time in it and a checksum of the whole list. A view always gets a time later than every
other, so the new views are the entries of the list in memory after that latest time; the roots
whose rows changed are checked against the copy too. When the checksum of the list equals the
copy's checksum brought up to date for those roots, nothing else changed; otherwise, as after
clearing Recently viewed or undoing that, the whole list is compared with the copy. A root whose
only change is that it entered or left a lifecycle, rating, tag or folder, or that its view time
changed, is recorded with the others in one statement for each of those fields, which compares the
stored values of the whole set in SQL. Every other entity is compared with what the synced values
show for it, and every field that differs is recorded. A renamed tag records `renamed_to`, and so
does a merged tag: a merge writes one row in `tag_merge` that names the tag it went to. So a tag
that another computer adds under the old name ends up under the new one; the auto-tags of folders
and the tag rules of smart folders follow the same forwarding. Lists keep the position keys of the
longest run of members that is already in order; the others get keys between their neighbours.
When a record makes an entity appear or disappear, the entities that refer to it are compared a
second time: the items and collections that have a tag or folder, the folders and smart folders
whose auto-tags or rules name it, the settings of a folder's view and the duplicate decisions about
an item. The records, the copies and the recorded revision are committed together, so after a
crash the recorder starts again from the revision it recorded.

`Library::apply_sync_records` runs the recorder, merges records from other computers into
`sync_value` and notes the entities whose visible state changed in `sync_pending`, in one
transaction of the sync file. For each field that the records change on such a waiting entity,
`sync_pending_value` keeps the value the field had before, which the library holds until the
entity is written. The entities are then written into the library in small transactions at
background priority. An item or collection whose new records only change its lifecycle, rating,
tags, name, notes, links, modification date, view time or sources, and a new item outside
collections and folders, are written directly into their rows and bitmaps, 32 in one transaction; a
new item is created as an import creates it. What the synced values show for these 32 is read from
the sync file before the transaction starts, and the tags that hold them are found on the thread
pool of the smart folders, so that the transaction holds the library's writer for only a few
milliseconds and a write of the user waits no longer than that. A view time that the projection writes is noted in
`changed_entity` for the recorder, because `recent_view` has no trigger. The others are written
with their structure, one in each transaction together with the waiting entities whose position
changed in the same folder: tags, tag groups, folders and smart folders are created and removed,
the folder tree, the smart folder tree and the library's settings are written as a whole when a
folder, a smart folder, the settings or a tag is written, and items and collections are written
with `restore_structure`, the code that undo uses for collections. A smart folder's rules name a
tag that is not visible as the tag it was renamed or merged into, or leave it out as deleting the
tag does, and name a folder that is not visible with the id 0, which matches nothing. The settings
of a view are written into its row of `view_pref`, also when its folder is created. A decision is
written into the duplicate pair of its two files once both items exist: the pair gets the status
Not duplicate, and is added with the distance 0 when the duplicate scan of this computer has not
found it; a decision that the synced values no longer show sets the pair back to Detected, as
undoing Not duplicate does. The items of a tag that was renamed or merged get the tag it went to in
the same transaction, also when they still wait. Items that the synced values no longer show are
deleted as Delete Permanently deletes them. A transaction is written only when the library has not
changed since the recorder's last pass; otherwise the recorder runs again first. When such a
transaction is the only write since the recorder's last pass and no entity waits to be revived or
recorded, the library shows what the synced values show for everything the transaction wrote. The
next pass then records nothing: it only stores the keys of the entities that the transaction
created and brings its copies of the bitmaps, the lists and Recently viewed up to the library,
without reading the entities that still wait. The time to write a batch therefore grows with the
size of the batch, and not with its square.

The recorder compares a waiting entity like any other, so a change that the user makes to it at any
time, also between two of these transactions, is recorded with a later timestamp, and the synced
values that are written later hold it. A pass reads from `sync_pending` and `sync_pending_value`
only the entities that it compares or that a compared value names, and the waiting roots only among
the roots that changed, so a pass after a write of the user costs what that write changed, also
while hundreds of thousands of entities wait. A value of a waiting entity is not recorded when it is still
the value from before the records (`sync_pending_value`), or when it is the value the synced values
store but do not show only because other entities that it names wait too, such as a cover that is
deleted; a tag of a waiting root is recorded only when the root entered or left that tag since the
last pass. A value that names a waiting entity that the library does not have yet is not recorded
either, and neither is the place of a shown folder or smart folder that equals its stored place:
the synced values can show such a folder elsewhere only because of other folders, such as a cycle of
parents that a waiting folder closes, and recording the same place again would make it the folder
whose place decides where that cycle is broken. Once the recorder records a field, its value from
before is dropped. Lists are compared
with the positions from before for their waiting members, and the transactions place entities in
folders the same way. If the computer stops after a transaction and before the sync file notes it,
the entities stay noted as waiting and are written again; what the transaction wrote is what the
synced values show, so nothing of it is recorded. An item or collection that comes back under its
old local id while the synced values delete it, as an undo of Delete Permanently brings it back, is
written in the same way with the latest synced values counted as visible, so what other computers
changed since is kept; the recorder records its full state after that, and compares each list that
holds it as a whole once it is written. A watched folder syncs with its path, and each computer
scans only the watched folders whose path exists on it; the path is therefore not unique in the
database, and `set_folder_watch` checks it instead. In the same way two smart folders in one parent
may have the same name in the database, and `create_smart_folder` and `update_smart_folder` check
the name that the user gives.

### Syncing through a cloud folder

The computers exchange their change records through a folder that the Google Drive or Dropbox
app copies between them (`picto-library/src/sync/cloud.rs`). `Library::turn_on_sync` makes a new
cloud library: it records the full state of the library into `sync_value`, stores the cloud
library's id and the path of that folder in `sync_state`, writes `picto/<library id>/library.json`
and writes the first snapshot. `Library::open_from_cloud` opens a cloud library in a new, empty
library on another computer: it turns sync on without recording anything and syncs, which adopts
the base snapshots and writes everything into the library. A library that has content is never
joined with a cloud library. `Library::set_sync_paused` pauses and resumes sync: while it is
paused the library records its changes and reads and writes no files. `Library::stop_sync_recording`
turns sync off; the cloud library stays in the folder. `core/src/cloud_sync.rs` has the same four
functions for the open library of the core.

In the cloud folder, each copy of the library writes into `devices/<device id>/`: its change files
`changes/<seq>.jsonl.zst`, numbered from 1 without gaps, and `device.json` with the name of the
computer, the version of Picto and the time of its last sync that read or wrote a change file. A
change file is JSON Lines compressed with zstd, one record `[kind, key, field, value, wall ms,
counter]` on each line and at most 100,000 records; the device id of the timestamps is the name of
the folder. A snapshot is `snapshots/<snapshot id>/`: shards `<nn>.jsonl.zst`, which hold the
records `[kind, key, field, value, wall ms, counter, device id]` of the entities whose key falls
into them, at most 100 of them with about 200,000 records each, and `manifest.json`, written last,
with the device and computer that wrote the snapshot, when, `covers` (for each device the last
change file whose records it holds), each shard's size and SHA-256 checksum, and
`pruned_originals`. A snapshot id is the time it was written, with 13 digits, a hyphen and a random
UUID. Every file is written under a temporary name, flushed and renamed. Only `device.json` is ever
written over.

A snapshot is valid when its manifest and every shard are there with the right size, and each shard
has the right checksum; the shards of a snapshot are read once in each run of the app to check
that, and their sizes are compared after that. Snapshots rank by the sum of their `covers`, then by
their id. The base snapshots are the valid snapshots that no higher-ranked valid snapshot covers,
and the synced state is their merge, every change file after the greatest of their `covers` for
its device, and the outbox (`picto-library/src/sync/snapshot.rs`). `sync_state` notes the base
snapshots that `sync_value` was built from. When the base snapshots differ, the sync adopts them:
one transaction of the sync file merges their records and the change files after them, removes the
synced values that none of them holds, and notes them and the change files read; the entities whose
visible state changed are then written into the library.

`Library::sync_with_cloud` first checks whether the database is a copy: the machine id that the
caller gives is not the one in `sync_state`, the device's folder holds the change file after the
last one the database noted as written, or a snapshot covers more of the device than that. That
happens when the library folder was copied to another computer, when a backup was restored, also one
older than the cleanup of its later change files, and when a sync stopped after it wrote a change
file and before it noted it. A copy waits until a snapshot is in the folder, takes a new device id,
whose records in its outbox take the new id in `sync_value`, and adopts the base snapshots. The sync
then adopts the base snapshots when they are not the ones it adopted, and reads the change files of
every other device in sequence order from the last one read, or from the adopted snapshots' `covers`,
and stops at the first one that is missing or cannot be read yet, as a file is while the cloud app
downloads it. A file that is gone because the cleanup deleted it is covered by the base snapshots,
which the sync then adopts. Each file is merged in one transaction of the sync file that also moves
`sync_read` for that device, so a crash merges the file again or not at all, and the merged entities
are then written into the library as described above. Next, the outbox is written as the next
change files; after each file is written, one transaction removes its records from the outbox and
notes its number. A crash between the two leaves a file that the next sync finds after the noted
number, so the database takes a new device id and sends the outbox again under it. The records are
then in two files with the same value and timestamps that differ only in the device id, which every
computer merges to the same value.

Last, the sync writes a snapshot when the newest base snapshot was written more than seven days
ago, the outbox is empty and no change file it needs is missing. It is the synced values with the
`covers` that the sync has read, which are then the cloud data, and it leaves out every field of an
entity that was deleted more than 30 days before, other than `deleted` and `renamed_to`, and every
reference, other than `place`, that is more than 30 days old and names such an entity or one the
synced values do not know. A `tag:<old>` field of a tag that went to another tag is written as the
field of that tag, with its timestamp, when it is the newer one. The items whose fields are left out
are listed in `pruned_originals`. Then the cleanup deletes the snapshots that a higher-ranked valid
snapshot written more than seven days ago covers, one at a time against those that remain, the
snapshots that have been incomplete for seven days since this computer first saw them so
(`sync_incomplete`), and the change files that every valid snapshot covers and whose files are more
than seven days old. The originals in `pruned_originals` are not deleted yet.

The core's sync worker (`library-sync` in `core/src/library_runtime.rs`) runs the recorder every
second while sync is on. While the library is connected to a cloud folder and sync is not paused,
it also syncs every five seconds, and in the second after the recorder recorded changes. The
machine id is a random id kept in the file `machine-id` of the app's data folder
(`core/src/cloud_sync.rs`). The sync uses the sync file's connection; it writes the library only in
the small background transactions of the projection, so library writes do not wait for it.

## Stored files and the pictures made from them

A library keeps its files in its folder `blobs` (`core/src/blob_store.rs`). Every file is stored
under the SHA-256 hash of its original's content. `<ab>` is the first two characters of the hash
and `<cd>` is the next two.

| Path | File |
| --- | --- |
| `blobs/f/<ab>/<cd>/<hash>.<extension>` | The original. |
| `blobs/t/<ab>/<cd>/<hash>.jpg` or `<hash>.png` | The thumbnail. The `.jpg` is used when it exists, and the `.png` otherwise. |
| `blobs/t/<ab>/<cd>/<hash>.first-frame.jpg` | For a video only: a picture of the video's first frame. |

The thumbnail and the first-frame picture are derived data. They are made from the original and
can be made again at any time, and no table of the database names them. The core and the app find
either one by building its path from the content hash, so no folder is searched.

**The thumbnail** fits inside 512 × 512 pixels. The grid, the inspector's preview, the Duplicates
screen, the picture of a drag and Quick Search draw it.

- The import of a still image makes the thumbnail before it publishes the item
  (`library_ingest_runtime::ensure_visible_thumbnail`). A subscription makes the thumbnails of the
  other files it downloads before it imports them.
- Every import also queues `thumbnail` work for the file in `work_item`. The derivative worker
  (`library_media_runtime::drain_batch`) makes the thumbnail when the file has none.
- When a library opens, `library_media_runtime::recover` queues that work for every original that
  has no thumbnail.
- The command `media.render_thumbnail_now` makes the missing pictures of one file at once. The
  app sends it for a tile whose thumbnail is missing and for Regenerate Thumbnail.
- A video's thumbnail is the frame at 35% of the video's length, read by `ffmpeg`. When that frame
  cannot be read, it is the first frame. An audio file's thumbnail is a PNG of its waveform.
- The app writes a `.png` thumbnail itself in two cases: it draws the thumbnail of a Flash movie
  or of a paged document, which the core cannot make, and Set as Thumbnail saves the frame that a
  video or a Flash movie shows.

**The first-frame picture** is made by the same `ffmpeg` call as a video's thumbnail, without a
seek, as a JPEG that fits inside 512 × 512 pixels (`library_media_runtime::make_first_frame`). A
video is a file whose MIME type starts with `video/`.

- The viewers draw it while a video loads: the media view, Quick Look, the detail window, the
  Inbox review, and a collection's reader for a video member (`viewer::picture_while_loading` in
  the app). Playback starts at the first frame, so the picture on screen does not change when the
  video's player draws its first frame. With the thumbnail it would change from the frame at 35%
  to the first frame.
- It is always the first frame, also when that frame is black. No other frame is tried.
- It is made whenever the video's thumbnail is made, and it then replaces an earlier one. Import
  and Regenerate Thumbnail therefore make both pictures.
- Whenever the `thumbnail` work or `media.render_thumbnail_now` finds the thumbnail in place and
  no first-frame picture, it makes the first-frame picture alone. A video that was imported
  before these pictures were kept gets its picture this way: a viewer that opens a video without
  one draws the thumbnail, as before, and sends `media.render_thumbnail_now` when it makes the
  video's player. The next time the video is opened, the viewer draws the picture. No pass over
  the library makes these pictures.
- When the picture cannot be made, the failure is written to the log, the work that made the
  thumbnail still succeeds, and the viewers draw the thumbnail.
- Set as Thumbnail changes only the thumbnail.

Both pictures follow the same rules outside the library's folder and at deletion:

- A Picto Pack holds only originals (`core/src/picto_pack.rs`). The import of a pack queues
  `thumbnail` work for each file, as every import does.
- A permanent delete queues the file in `blob_cleanup_queue`, and the cleanup deletes the
  original, the thumbnail and the first-frame picture together (`BlobStore::delete`).

## Subscriptions

Subscriptions store their definitions, runs, source posts, source items, retries and progress in
SQLite. A user signs in to a site in a sign-in window that the app opens on the system's web view
(`app/src/logins/`). The login that results is kept in the operating system's credential store.

Every source uses one path, written in Rust. Neither the app nor a package holds a Python
runtime, gallery-dl, OF-Scraper or any other helper program for sources:

`query -> provider adapter -> shared HTTP runtime -> one exposed post -> current-post downloads ->
canonical ingest -> settlement acknowledgement -> next post`

A provider (`picto-sources/src/providers/`) does only what differs from site to site: it chooses
the addresses to request, reads the pages of results up to a limit and the cursors between them,
parses the responses and describes the media files. The shared runtime does the rest: the
schedule, the pace of the HTTP requests and their retries, the downloads, the staging, the
import, the counters and the settlement. The headers, logins and parsing of one provider must not
change what another provider does. A test checks that the list of sites in
`core/src/subscriptions/sites.rs` and the list of providers are the same.

### One-post settlement

For each query, in order:

1. Expose the next source post and record it as traversed.
2. If it has no usable media, settle it as skipped.
3. Download every usable file of that post (files within the post may download concurrently;
   media of later posts is never prefetched).
4. Ingest the post canonically, then settle it as added.
5. Only after settlement is acknowledged, expose the next post.

An adapter may fetch a bounded metadata page when its API paginates, but the session refuses to
expose another post until the current one settles.

- **Limit.** `Posts per run` caps settled posts per query per run: both added and skipped posts
  count toward it (`SourcePostOutcome::counts_as_settled_post`). A failed or interrupted post does
  not settle and does not advance the cursor. Partitioned feeds (for example OnlyFans) share one
  query-level budget with independent cursors.
- **Multi-file posts.** With **Group multi-media posts** on (the default, per query), the files of
  one post are buffered until the post completes and ingested as one collection. With it off
  (**Keep post media separate**), each file becomes its own standalone item. A single-file post is
  always a standalone item. E-Hentai/ExHentai galleries are one post and always one collection.
- **Duplicates.** Exact content hashes reuse existing media facts instead of creating another
  standalone root.
- **ZIP attachments** go through the shared bounded extractor and become members of the current
  post (1 GiB archive and expanded total, 512 MiB per entry).
- **Runs.** A subscription has at most one active run. Staging is scoped to the run and removed
  after settlement, failure, startup recovery, or reset. Restart resumes from the last settled
  post; stable source identity and content hashes keep a repeated post idempotent.

A run counts three numbers: the posts traversed, whose metadata was read; the posts added, all of
whose media files reached the library; and the posts skipped, which were settled without a new item
in the library. A post is counted as added only after its import has committed.

The totals that the subscription list shows over all runs are computed from the stored results
of the runs: the distinct source posts added and source items retained for each query, and the
distinct posts added across all queries of a subscription. Deleting media or a later failed
download does not lower these totals; only resetting the subscription's history does. When a
library opens, its writer builds an index of these results in TEMP tables
(`picto-library/src/subscription_counts.rs`). Triggers keep a reference count for each distinct
post and item inside the same transaction as each write, and the small totals are published with
the in-memory snapshot through `published_write`. A file that is downloaded again successfully
adds a reference, not a count, and refreshing the subscription list never adds up the stored
attempts. When a run or query is deleted, the cascade also removes its references, even when the
parent row has already been deleted. A rolled-back transaction or savepoint also rolls back the
TEMP tables. The index is never stored in the database or in the checkpoint, and a maintenance
write that would change it is rejected. There is no second stored counter and no schema
migration for it; the scan of the stored results is done once at every open.

When a source breaks, follow
[`SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md`](./SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md).
