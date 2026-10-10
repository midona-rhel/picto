# Picto Architecture

Picto is one Electron application. React renders the interface, Electron owns operating-system
integration, and the Rust NAPI module owns library behavior and persistence.

The production path is direct:

`IPC command -> application operation -> SQLite transaction -> projection update -> invalidation`

SQLite is authoritative. Roaring bitmaps accelerate root-item scopes and can be rebuilt. Every
committed mutation settles its projections, increments the library revision, and emits affected
resource categories. Frontend consumers re-query those resources instead of predicting backend
state or patching counts locally.

A visible root is standalone image/video media or an ordered collection. Physical files, logical
media occurrences, and visible roots have separate identities. Attached collection members inherit
their root's lifecycle and folder placement. `All` contains active roots only; Inbox and Trash are
separate lifecycle scopes.

Manual imports, watched folders, subscriptions, and retries enter one durable ingest path. See
[Subscriptions](#subscriptions) for how source posts become standalone items or collections.

## Application boundaries

- React owns presentation and transient interaction state. It does not open SQLite or infer whether
  a native operation committed.
- Electron owns windows, menus, protocols, updater delivery, credentials, and packaged native tools.
  The preload exposes a narrow typed IPC surface; renderer code has no Node integration.
- Rust owns library sessions, queries, mutations, background work, and invalidation. A generation
  change closes the old library before a new session can answer commands.
- Long-running work is durable and queryable. UI progress is a projection of persisted work rather
  than an independent queue.

## Prepared library views

`items.window` prepares an ordinal or normalized-scroll destination on one captured projection
snapshot and never opens a SQLite read for it: matching, ordering, aggregates, summaries and
previews all come from the resident library model described below. The result owns the window,
its resolved start, the first four query results, exact counts and one revision. Sequential consumers retain
`items.query` cursors; grid navigation and refreshes do not fetch a separate preview page.

The grid controller separates pending navigation from the displayed session. Outgoing scroll
events and background refreshes cannot supersede a pending destination. Navigation preparation
runs during the existing 170 ms fade-out. The destination is revealed after valid geometry and
primary colors paint, without waiting for thumbnails. Refreshes keep the displayed grid while
preparing one replacement, then coalesce newer invalidations rather than retrying until the
database stops changing. The 10-second request deadline also covers preference loading.

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

The renderer holds only a ~1,500-tile window, so a selection is never expanded from loaded tiles
when it reaches beyond them. Select-all is a `query` target with exclusions. A shift-click whose
anchor has left the window is a `range` target carrying both roots and their view positions;
the backend reads that ordinal span straight from the resident order and trusts it only when
the roots at both ends still match, otherwise it locates the two roots in the complete order.
A shift-click inside the loaded window stays an explicit list.

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
and reports `library open cancelled`; the shell exposes the stage in flight (migrating, loading,
starting) through `library.open_status` so the loading screen can say what is happening. Nothing else is persisted as derived state: there are no
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

Receipts stay precise. Item ids qualify only `library`; bookkeeping receipts (tasks,
subscriptions, settings) never widen a frame to a broad change. `smart_folders` is announced only
when a folder's result or rule actually changed. Derivative facts publish narrowly: a perceptual
hash announces nothing and a palette refreshes only the colour index. The IPC response cache for
`items.query` and `items.mime_facets` is keyed on the query epoch, which bookkeeping does not move.

Background workers (ingest, FTS, derivatives, AI tagging, subscriptions) sleep until the library
publishes a change, with a fallback probe every 2-30 s for work that becomes ready with time alone
(`core/src/work_wake.rs`). An idle library starts no writer transactions between probes. Imports
publish prepared items in groups of up to 48, flushed at least every 250 ms so the grid fills
progressively. Auto-tagging on import writes in batches outside the undo history; manual AI tagging
stays undoable.

`picto-library/examples/library_ops_benchmark.rs` measures these operations on a synthetic library
(`-- [roots] [runs] [smart_folders]`, default 1M roots and 100 smart folders). Its output is local and
never committed.

Committed grid invalidations coalesce in the grid controller with one active refresh and one
pending successor. Fast refreshes use a 40 ms delay. During sustained expensive refresh work,
the cooldown is three times the previous refresh duration (capped at one second), allowing
background work to progress without imposing a fixed two-second stale-view interval on edits.
The displayed grid remains intact during refresh, and navigation retains its existing fades.

## Subscriptions

Subscriptions persist definitions, runs, source posts, source items, retries, and progress in
SQLite. Direct site sessions are captured in a Picto-owned browser and secrets remain in the
operating-system credential store.

Every source uses one native Rust path; there is no Python runtime, gallery-dl, OF-Scraper, or
sidecar in production or packaging:

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

When a source breaks, follow
[`SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md`](./SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md).

## Cloud sync

Cloud sync (experimental) consumes committed revisions and application operations; it does not
write SQLite, blob files, or Roaring projections behind the core.
