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

Manual imports, watched folders, subscriptions, and retries enter one durable ingest path. Source
adapters normalize posts into ordered media items. A multi-media post is promoted to a collection
when its second distinct item arrives, without delaying the first item.

Subscriptions persist definitions, runs, source posts, source items, retries, and progress. Direct
site sessions are captured in a Picto-owned browser and secrets remain in the operating-system
credential store.

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
scan the resident text shards, in parallel above 32 shards. There is no admission budget, no
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

Committed grid invalidations coalesce in the grid controller with one active refresh and one
pending successor. Fast refreshes use a 40 ms delay. During sustained expensive refresh work,
the cooldown is three times the previous refresh duration (capped at one second), allowing
background work to progress without imposing a fixed two-second stale-view interval on edits.
The displayed grid remains intact during refresh, and navigation retains its existing fades.

## Pre-release integration seams

Cloud Sync must consume committed revisions and application operations; it must not write SQLite,
blob files, or Roaring projections behind the core. Tutorials may observe stable navigation and
command identifiers, but may not fork production components or synthesize host input. Both are
release gates until their own behavior, persistence, accessibility, and packaged smoke tests pass.
