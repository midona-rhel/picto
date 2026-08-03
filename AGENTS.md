# Product Contract

Imaginator is an Electron media library backed by SQLite. Roaring bitmap projections make
media-entity lookup and scope filtering fast; SQLite remains the source of truth.

Media entities are images, videos, or image collections. Collections contain images only and
are aggregates over their children rather than second owners of child metadata.

The product must support imports, subscriptions, duplicate review, automatic tagging, tag
management, folders, smart folders, search, untagged and uncategorized scopes, recently viewed
media, and cloud sync.

Cloud sync writes library metadata changes and media blobs into a user-owned folder already
synced by a desktop provider such as Google Drive or Dropbox. The provider transports files;
Imaginator does not need its own cloud account system or provider-specific upload API.

# Engineering Rules

- Describe each behavior in a few sentences before implementing it. If that is not possible,
  stop and clarify the behavior instead of adding architecture.
- One user behavior gets one production path. Different UI or source adapters normalize into
  that path; they do not reimplement it.
- Prefer less code and direct, tested behavior. Delete replaced paths in the same change.
- Do not add compatibility shims between this app's frontend and backend. Compatibility belongs
  only in explicit database migrations or external-format adapters.
- Before 1.0, do not write database migrations. A library is created at the current schema or must
  match it exactly; incompatible databases fail clearly and are never mutated or deleted automatically.
- Production code must not retain TODO/FIXME placeholders. Implement the behavior, remove it, or
  identify it as a concrete release blocker.
- Tests prove user-visible behavior and persistence boundaries, not internal architecture.
- PBIs describe observed unfinished behavior with a finite acceptance test. Archive them when the
  behavior works; do not create PBIs to postpone cleanup.

# Release Completion

The executable release backlog is `docs/RELEASE_COMPLETION_PLAN.md`. Work follows its dependency
order: clean integration, truthful migrations and verification, core behavior, subscriptions,
duplicates, tag management, AI tagging, folder-based sync, deletion, then packaged release proof.

- Do not start broad feature work while the Git index has unresolved entries.
- Agents receive bounded, disjoint write scopes. They do not stage or commit; the integration owner
  reviews, verifies, and commits each coherent slice.
- A PBI closes only after its focused tests and application-level smoke pass.
- Remove unreachable legacy code, fake verification, no-op UI, unused dependencies, and superseded
  documentation instead of preserving them for compatibility.
- Do not split working modules to satisfy arbitrary size limits. Consolidate only duplicate behavior
  or competing production paths.
