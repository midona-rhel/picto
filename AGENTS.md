# Product Contract

Picto is an Electron media library backed by SQLite. SQLite is the source of truth; Roaring
bitmaps are rebuildable projections used to keep lookups fast.

Visible library items are either standalone media items or collections. Media assets may be images,
video, audio, 3D, design-source, font, RAW, document, or web files. Acceptance does not imply that a
detail renderer exists yet; unsupported previews retain the original bytes and metadata. ZIP files
are safely expanded into a collection and are never stored as opaque library items. A collection is
a first-class root item: it owns lifecycle and folder membership, and
its members inherit both. Collection operations apply to all members unless a member is detached.
`All` is the accepted active library only; Inbox and Trash are separate lifecycle scopes and must
not appear in All, folders, smart folders, library search, or those counts.

The product must support imports, subscriptions, duplicate review, automatic tagging, tag
management, folders, smart folders, search, untagged and uncategorized scopes, recently viewed
media, collections, and deletion. Cloud sync is deferred from this release.

Every subscription source uses one direct-site authentication flow. Picto opens the source's real
login page in a Picto-managed browser, captures the resulting session, and stores it in the OS
credential store. Product UI must not ask users to paste passwords, cookies, tokens, or API keys.
Sources that support anonymous access may still run without logging in.

Picto targets libraries from hundreds of thousands up to roughly one million media assets. Common
writes must update affected projections incrementally and keep the UI responsive.

# Engineering Rules

- Describe behavior briefly before implementing it; clarify instead of adding architecture when
  the behavior is not clear.
- One user behavior gets one production path. Adapters normalize into that path.
- Prefer direct, tested behavior and delete replaced paths in the same change.
- Do not add frontend/backend compatibility shims.
- Before 1.0, do not write database migrations. New libraries must match the current schema;
  incompatible libraries fail clearly without mutation.
- Tests prove user-visible behavior and persistence boundaries, not forwarding between wrappers.
- Production code must not retain TODO/FIXME placeholders; implement, remove, or record a concrete
  release blocker.
- PBIs describe finite unfinished behavior and are deleted when the behavior works.

# Backend Contract

User mutations follow one path: IPC command, application operation, SQLite transaction, projection
settlement, and one compact revision/resource invalidation. The event contains a revision, affected
resource keys, and affected item IDs. Consumers re-query canonical data; the backend does not send
renderer-specific count patches, sidebar patches, or speculative grid inserts.

Subscriptions use one durable persisted worker for scheduled runs, manual runs, retries, and
restart recovery. Progress is persisted and queried; interrupted work resumes idempotently.

# Host Input Automation Safety

- Never use macOS System Events, `osascript`/AppleScript, Accessibility automation, or synthetic
  host-wide input for Picto audits or tests.
- Prefer source inspection and read-only Electron CDP/DOM queries or screenshots.
- If UI driving is unavoidable, obtain explicit approval, run one bounded process, record its PID,
  and guarantee cleanup on success, failure, or interruption.

# Release Completion

The executable backlog is `docs/RELEASE_COMPLETION_PLAN.md`. Do not start broad feature work while
the Git index has unresolved entries. Agents receive disjoint write scopes and do not stage or
commit; the integration owner reviews and commits coherent slices.

A PBI closes only after focused tests and an application smoke pass. Remove unreachable code, fake
verification, no-op UI, unused dependencies, and superseded documentation instead of preserving
them for compatibility.
