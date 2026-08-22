# Product Contract

Picto is an Electron media library backed by SQLite. Roaring bitmap projections make root-item
lookup and scope filtering fast; SQLite remains the source of truth.

A visible library item is either standalone image/video media or an ordered collection of media.
The physical file, logical media occurrence, and visible root item are separate identities. A
collection is a root item: its members have no independent lifecycle or folder membership while
attached. Collections never contain other collections.

`All` means the accepted library, not every stored item. Only active roots belong to it. Inbox roots
are awaiting acceptance; Trash roots are awaiting deletion or restoration. Inbox and Trash roots
must not appear in `All`, folders, smart folders, library search results, or any of their counts.

Tags and content metadata belong to media. A collection write applies to every member and a
collection read aggregates its members. Detaching a member creates a root that inherits the
collection lifecycle and folders. Ungrouping creates roots for every member. Permanently deleting a
collection deletes all members and physical files that are no longer referenced.

The product must support imports, subscriptions, duplicate review, automatic tagging, tag
management, folders, smart folders, search, untagged and uncategorized scopes, and recently viewed
media. Cloud sync is not part of this release and no disabled sync implementation remains.

Every subscription source uses one direct-site authentication flow. Picto opens the source's real
login page in a Picto-managed browser, captures the resulting session, and stores it in the OS
credential store. Product UI must not ask users to paste passwords, cookies, tokens, or API keys.
Sources that support anonymous access may still run without logging in.

Picto targets libraries from hundreds of thousands up to roughly one million media assets.
Common interactive writes must not rebuild or transmit the whole library. Unavoidable bulk work
may take seconds, but must be measured on representative data and keep the UI responsive.

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
- A reviewed one-time development conversion tool may be used to move the current active test library
  at an agreed cutover point. It is not shipped, is never run automatically, and is deleted after the
  converted library passes verification.
- Production code must not retain TODO/FIXME placeholders. Implement the behavior, remove it, or
  identify it as a concrete release blocker.
- Tests prove user-visible behavior and persistence boundaries, not internal architecture.
- PBIs describe observed unfinished behavior with a finite acceptance test. Delete them when the
  behavior works; Git history is the archive. Do not create PBIs to postpone cleanup.

# Host Input Automation Safety

- Never launch or use macOS System Events, `osascript`/AppleScript, Accessibility UI automation,
  synthetic keyboard or mouse events, or any tool that can capture host input for Picto audits or
  tests. Use ASAR/source inspection and read-only Electron CDP/DOM queries or screenshots instead.
- If UI driving is absolutely necessary, obtain explicit user approval before starting it. Run one
  bounded process with a timeout, record its PID, and guarantee termination and cleanup on success,
  failure, or interruption. Never leave System Events running.
- Emergency recovery: identify and terminate only the automation PID or System Events instance
  started by this task; do not kill broad or unrelated processes.
- This rule follows an observed incident where System Events launched during a visual audit captured
  host-wide keyboard and left-click input until its PID was terminated.

# Release Completion

The executable release backlog is `docs/RELEASE_COMPLETION_PLAN.md`. Work follows its dependency
order: backend replacement, core behavior, subscriptions, duplicates, tag management, AI tagging,
deletion, then packaged release proof.

- Do not start broad feature work while the Git index has unresolved entries.
- Agents receive bounded, disjoint write scopes. They do not stage or commit; the integration owner
  reviews, verifies, and commits each coherent slice.
- A PBI closes only after its focused tests and application-level smoke pass.
- Remove unreachable legacy code, fake verification, no-op UI, unused dependencies, and superseded
  documentation instead of preserving them for compatibility.
- Do not split working modules to satisfy arbitrary size limits. Consolidate only duplicate behavior
  or competing production paths.
