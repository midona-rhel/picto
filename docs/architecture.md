# Picto Architecture

Picto is one Electron application. React renders the interface; Electron owns windows, protocols,
and operating-system integration; the Rust NAPI module owns library behavior and persistence.

SQLite is the source of truth. Roaring bitmaps, sidebar rows, search indexes, thumbnails, colors,
and duplicate candidates are derived data that can be rebuilt. Public actions cross one typed
frontend API and one Rust dispatch path; background work uses durable SQLite queues.

A media entity is an image or video. `All` is the accepted library and contains active entities
only. Inbox and Trash are separate lifecycle scopes and never contribute to normal folders, smart
folders, search, or counts.

Imports from drag-and-drop, watched folders, subscriptions, and retries normalize into the durable
ingest queue.
Subscription adapters discover posts and download files; the same ingest queue creates one media
entity per file, with shared source-post metadata and source order on each entity. It never creates
an aggregate post entity, hidden group, or automatic folder.

Future grouping or rearrangement, if needed, may be represented by a dedicated external media
manifest or file format. The current data model contains no grouping abstraction, placeholder,
hidden group, or extension hook.

A subscription is one top-level scheduled run unit containing one or more source queries. Queries
own source cursors and provenance; no subscription-group layer exists.

Folder sync writes immutable blobs and versioned metadata operations into a user-selected folder.
Google Drive, Dropbox, or similar desktop software transports that folder. Each device retains its
own SQLite database, credentials, queues, thumbnails, and operational history.
