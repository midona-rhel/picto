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
credential store. Cloud sync is not part of this release.
