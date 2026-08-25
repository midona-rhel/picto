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
- Electron owns windows, menus, protocols, updater delivery, credentials, and packaged sidecars.
  The preload exposes a narrow typed IPC surface; renderer code has no Node integration.
- Rust owns library sessions, queries, mutations, background work, and invalidation. A generation
  change closes the old library before a new session can answer commands.
- Long-running work is durable and queryable. UI progress is a projection of persisted work rather
  than an independent queue.

## Pre-release integration seams

Cloud Sync must consume committed revisions and application operations; it must not write SQLite,
blob files, or Roaring projections behind the core. Tutorials may observe stable navigation and
command identifiers, but may not fork production components or synthesize host input. Both are
release gates until their own behavior, persistence, accessibility, and packaged smoke tests pass.
