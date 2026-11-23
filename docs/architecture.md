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

A visible root is standalone image/video media or an ordered collection. Physical files, logical
media occurrences, and visible roots have separate identities. Attached collection members inherit
their root's lifecycle and folder placement. `All` contains active roots only; Inbox and Trash are
separate lifecycle scopes.

One file is exactly one media item, enforced by a unique index on `media_item(file_id)`. Every
import resolves a file it already holds to that item: a standalone import adds its tags and
provenance to it, a loose item joins a new collection that contains it, and an item already in
another collection causes the overlapping sets to combine. Chained overlaps also combine,
retaining every unique media item and merging tags, folders, notes, source links and the highest rating. Resolving a duplicate by
keeping one file merges the other item into the kept one (tags, folder places, the higher rating,
the lifecycle that keeps it most, notes and source links), undoably. A merged-away item's stable
key stays in `library_item_alias`, so re-imports, Picto Packs and cloud copies resolve to the item
that stays. Libraries from before the rule merge their shared files once at open
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

`items.window` prepares an ordinal or normalized-scroll destination on one captured projection
snapshot and never opens a SQLite read for it: matching, ordering, aggregates, summaries and
previews all come from the resident library model described below. The result owns the window,
its resolved start, the first four query results, exact counts and one revision. Sequential consumers retain
`items.query` cursors; grid navigation and refreshes do not fetch a separate preview page.

Window cancellation has one scoped backend execution context: Rust text/tree/numeric ordering,
resident summary hydration and JSON serialization observe it. Parallel text workers explicitly
receive the token. Match membership and aggregates have independent cache dependencies;
changing byte totals does not invalidate unrelated text membership.

Text filters and text-based smart folders share one matching contract: the trimmed, lowercased
literal of at least three characters is a byte substring of the requested lowercased fields.
Selections of up to 4,096 roots look each root's settled text up directly; larger selections
scan the resident text shards, in parallel above 32 shards. A shard scan runs one vectorized
substring search (`memchr::memmem`) over the shard's text and maps each hit to its row and field,
resuming at the next row in scope; shards with no root in scope are skipped, and a shard with
under 3% of its rows in scope looks those rows up instead. A query that contains a recent
selective query (typing one more letter) searches only that query's matches. FTS settlement takes
the oldest 256 dirty roots per batch through an index, so its cost does not grow with the
backlog. There is no admission budget, no
trigram candidate probe and no SQL fallback; a common term over a million items is a visible
resident scan, not a hidden slower path. Every byte of the settled text is ordinary, including an
embedded NUL: text after it stays searchable and a query containing one matches the fields that
contain that byte sequence, while alphabetical order keeps SQLite's NOCASE rule of stopping at a
NUL. The canonical `root_fts` table remains the settled-text source, and a full-table `instr()`
scan of it is the exactness oracle the resident matcher is tested against.

Captured/imported date windows use signed bit-sliced projections, retaining pre-epoch
timestamps rather than clamping their ordering keys. Missing captured dates share the `-1`
sort bucket with real `-1` timestamps, with root ID breaking ties in either direction.
Filtering retains its existing nonnegative date-range semantics and excludes missing dates.
Folder-tree order uses the same direct descending imported-date selection. These paths select
only the requested ordinal window. Sequential `items.query` cursors page the same resident
orders: value-keyed cursors for date, size and recently-viewed order seek their position in one
cached complete order, and name, rating, random and folder-order cursors keep their pagers.
The derived checkpoint implementation version distinguishes signed date keys from older
clamped keys; incompatible derived checkpoints rebuild from canonical SQLite data.

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
Drawing facts live in immutable 1,024-root shards that intern each shard's content hashes and
MIME strings; names come from the alphabetical index rather than a second copy. A window
hydrates its summaries from these records and the bitmaps alone. Detail requests, media
records and duplicate inspection still read canonical tables, the last inside a consistent read
that captures the snapshot it will combine with.

Opening a library rebuilds the text, name, drawing and navigation state from canonical rows in
streaming scans, then reuses the bitmap-and-index checkpoint when it is valid for the exact
database revision or rebuilds that too. The open runs under a scoped cancellation token with no
deadline: a later open, a close or process shutdown cancels it, the migration's statements are
interrupted through SQLite's progress handler, and the resident scans stop at their existing
check points. A cancelled open commits nothing, removes the backup its migration attempt took,
and reports `library open cancelled`; the core reports the stage that is running (migrating,
loading, starting) through `library.open_status`, so that a loading screen can name it. Nothing else is persisted as derived state: there are no
drawing pages, text chunks or name nodes on disk, no per-domain format versions, generations or
repair passes, and no helper reader connections. The supported predecessor migration backs up
the database, drops those tables and redefines the one journal; a first open after migration or
after any open pays the resident build, which the startup statistics report stage by stage.

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

Resident memory is a measured tradeoff, not a quota: the snapshot's own estimate counts each
shared allocation once per snapshot it appears in, and the memory probe reports retained
snapshots separately from the live model.

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
only when its membership actually moved. `smart_settle_oracle` checks this against a fresh
evaluation of every folder after each step of a random edit sequence.
Results cover roots in every lifecycle; a smart-folder scope and its sidebar count intersect them
with Active on read, as every other scope does. Trashing, restoring or accepting Inbox items
therefore re-checks no smart folder, however many exist.

Every SQLite connection sets its own page cache (64 MB for the writer, 16 MB per reader) instead
of relying on the operating system's file cache, which differs widely between Linux, Windows and
macOS; memory-mapped I/O is off. Indexes that only accelerate queries live in `accelerator_indexes.sql` and are created at create
and open; adding one needs no fingerprint migration. Autocheckpoint is off: the runtime folds the
WAL back every minute and truncates it on close.

### Writes, history and background work

Every canonical write goes through `Library::publish` (in `picto-library/src/mutation/`, one file
per aggregate: tags, folders, smart folders, imports, duplicates, structure, undo). It opens the
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
records no entry and frees its files at once; the app asks the user to confirm a reject that
cannot be undone, and it counts the items with the selection that the library resolved, not with
the items the grid has loaded. History protection is registered
before releasing the writer, and cleanup reads it after acquiring the same writer. Undo restores
recent views and cascaded queue jobs, assigning new queue IDs and requeuing interrupted running jobs.
When the app has told the user that a delete can be undone, it passes `require_undoable`; the
serialized writer checks the resolved selection against the limit and refuses the delete if it
no longer fits. Only a fresh
nonundoable confirmation permits a retry without that constraint. Delete requests await both
confirmation and persistence, returning success only after the mutation commits; cancellation
or failure never advances Inbox review.

Receipts stay precise. Item ids qualify only `library`; bookkeeping receipts (tasks,
subscriptions, settings) never widen a frame to a broad change. `smart_folders` is announced only
when a folder's result or rule actually changed. Derivative facts publish narrowly: a perceptual
hash announces nothing and a palette refreshes only the colour index. The core's response cache for
`items.query` and `items.mime_facets` is keyed on the query epoch, which bookkeeping does not move.

Background workers (ingest, FTS, derivatives, AI tagging, subscriptions) sleep until the library
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
merge has finished or has been undone. The other library's subscriptions, cloud state, watched
folders, settings, recent views and duplicate decisions are not read.

`picto-library/examples/library_ops_benchmark.rs` measures these operations on a synthetic library
(`-- [roots] [runs] [smart_folders]`, default 1M roots and 100 smart folders). Its output is local and
never committed.

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

- Cloud sync uploads and downloads only the originals in `blobs/f`
  (`core/src/cloud/snapshot.rs`). When it has transferred an original that has no thumbnail, it
  queues `thumbnail` work for it, and that work makes both pictures.
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

Provider adapters (`picto-sources/src/providers/`) own only endpoint selection, bounded
pagination and cursor decoding, response parsing, and media descriptors. The shared runtime owns
scheduling, HTTP pacing and retries, downloads, staging, ingest, counters, and settlement. One
provider's headers, authentication, or parsing must not affect another. The product site registry
(`core/src/subscriptions/sites.rs`) and the native registry are tested for exact equality.

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

Progress counters: posts traversed (metadata inspected), posts added (complete media set reached
the library), posts skipped (settled without a new visible item). A post is never reported as
added before canonical ingest succeeds.

Lifetime catalog counts belong to canonical run outcomes: distinct added source posts and
retained source items per query (and distinct added posts across a subscription's queries).
Deleting media or failing a later download does not erase earlier outcomes. Explicit history
reset does. The library writer builds a disposable TEMP outcome index on open, maintains its
distinct-member reference counts with transactional triggers, and publishes the small totals
with the resident snapshot through `published_write`. Successful redownloads add references,
not counts; workspace refreshes never aggregate historical attempts. Cascades remove the
original indexed attribution even when the parent has already gone. Rollback/savepoints roll
back the TEMP index too. The index is neither persisted nor checkpointed, and maintenance
writes that would change it are rejected. No second canonical counter or schema migration is
introduced; open pays the history scan once per session.

When a source breaks, follow
[`SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md`](./SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md).

## Cloud sync

Cloud sync is experimental. It uses a folder that the Google Drive or Dropbox desktop app syncs;
Picto has no account service and signs in to nothing. The core owns it (`core/src/cloud/`), and it
changes a library only through the library's own writes.

A library writes below `picto/<library id>/` in the cloud folder:

| Path | Content | Written |
| --- | --- | --- |
| `library.json` | the library's id and name | once, by the first sync |
| `blobs/f/<aa>/<bb>/<hash>.<ext>` | the original files, by content hash | when a file is missing there |
| `snapshots/<id>.sqlite.zst`, `<id>.json` | a compressed copy of the whole database, and its manifest with both checksums | by every sync |
| `entries/<device id>/<xx>.<digest>.jsonl.zst` | the entries that this computer wrote | by a sync after this computer wrote or changed one |

A sync (`snapshot::publish_library`) runs when the library has changed and has been idle for 30
seconds, and on Sync Now. Every file is written beside its target and then renamed onto it, so a
reader never sees half a file. Open from Cloud… takes the newest snapshot as the new library's
database, gives the library a device id of its own and copies the originals. Restore replaces the
database with one snapshot.

### Entries

Each computer records what is done to its library as entries in `shared_entry`
(`picto-library/src/mutation/shared.rs`), sends them to the other computers through the cloud
folder, and changes its library to match the entries of all computers. A picture is identified by
the content hash of its file. A tag is identified by its namespace and name, and a folder by its
stable key.

The entry kinds are `add` (one per file and computer, with a record of the picture's name, notes,
rating, status, tags and folders), `delete`, `name`, `notes`, `rating`, `status`, `item_tag`,
`folder_item`, `tag_delete` and `folder_delete`. For each kind and key the library keeps only the
latest entry, so the same entries give the same library in any order. If one computer sets 3 stars
at 10:00 and another sets 5 stars at 10:05, both end with 5 stars. The clock is the later of the
wall clock and the latest time the computer has seen (`cloud_state.hlc_physical_ms`,
`hlc_logical`), and all entries of one action share one time.

`shared_view.rs` works out a picture from its entries. It exists when an `add` is later than its
`delete`. A field comes from its latest entry after the deletion, or else from the earliest `add`
that counts. A tag is on the picture when its latest `item_tag` says so, or else when an `add`
record has it, and it goes when the tag was deleted after it was put on: a tag put on at 9:00 and
deleted at 10:00 is off, one put on again at 10:05 is on. A deleted folder never holds anything
again.

An import writes an `add` only when no computer has added the file yet; otherwise the picture is
changed to look the way the entries say. Undoing a permanent deletion always writes an `add`, and
so does a repaired video for its new file. Deleting a picture, merging a duplicate away and
repairing a video write `delete` for the file that goes.

A library starts recording at its first sync (`Library::start_recording`), and every file it holds
then gets an `add` marked as written at the start, so its tags count as older than any deletion of
those tags. After that, `record_changes` compares the library before and after each published write
(from `PublishedDelta::settle`) and stores one `pending_entry` row per kind of change, with the
changed roots as a bitmap, so tagging a million pictures costs one row. `Library::settle_entries`
turns the rows and the files of the start into entries in batches of 20,000. Deletions of files,
tags and folders are written as entries at once.

A merge (`Library::merge_entries`, `shared_merge.rs`) first turns this computer's pending rows into
entries, keeps the entries that are later than the stored ones, and changes every picture they
touch through the functions that Undo and Redo use, in one published write without an undo step.
It writes no entry and doesn't move the clock past the entries it saw. A picture that exists by the
entries and isn't on this computer is listed by `Library::wanted_files`; fetching those files isn't
built yet. Pictures in collections, and collections, get no entries and are left alone by a merge.

A computer's entries are spread over 256 files in `entries/<device id>/` by the first byte of the
SHA-256 of an entry's key (`core/src/cloud/entries.rs`). A sync rewrites only the files whose
keys changed since the time in `cloud_device_frontier`. A file's name holds the first 16 hexadecimal
digits of the SHA-256 of its content, so a file is never changed; a new state is a new file and the
old one is removed. The content is zstd-compressed JSON lines after a header with the library, the
computer and the whole SHA-256. If any waiting file doesn't match its name and header, nothing is
merged and the sync state becomes `error` with the file's name. A file that can't be read yet waits
for the next round.

The original of a permanently deleted picture is removed from the cloud folder by the computer that
deleted it, after the "Keep deleted files" time (`cloud_deleted_original`,
`purge_expired_cloud_blobs`).

`scripts/sync-model/` holds the Python simulation that these rules were checked with, including the
parts that aren't built yet (collections, import tasks, snapshots, Restore and originals).
`model2.py` runs three to five computers that act and sync in random order and checks that they end
with the same library, that nothing is lost that nobody removed, and that the later action wins.
`round2_runs.sh` and `final_runs.sh` are the searches that were run, and `replay2.py` replays one
run. Run it with `python3 -I` before changing a rule. `picto-library/tests/shared_entries.rs`
checks the same properties on three real libraries.

`cloud.data.delete` (Delete Cloud Data… in the Library Manager)
removes the library's folder from the cloud folder and then switches sync off
(`DirectoryProvider::remove_library`). It removes `library.json` first, and after it only files
whose name and place are those of a file that the sync writes. A file of any other name stays, and
so does every folder that still holds something. A sync, the merge of the entries and the
removal hold one lock, so the data is not removed while a sync writes into it.

A device whose library has synced with the folder before and no longer finds `library.json` there
treats the cloud data as removed: it switches its sync off, leaves the message "The cloud data of
this library was deleted. …" in `cloud_state`, writes nothing into the cloud folder and changes
nothing else (`cloud::stop_if_cloud_data_removed`). Connecting a library to a cloud folder clears
the time of the last sync and the clock time up to which the computer's entries were written, so
the library then writes all of them into the folder's cloud data.

Earlier versions wrote tombstone files into `tombstones/`. This version doesn't read them, and a
library that it opens drops its tombstones and starts recording at its next sync.
