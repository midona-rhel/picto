# Picto Release Completion Plan

## Goal

Finish Picto around one understandable backend: SQLite truth, rebuildable bitmap projections, one
application operation path, one compact invalidation contract, and one durable subscription worker.
Collections are first-class library items. Cloud sync is deferred from this release.

## Release Rules

1. Visible library items are standalone media or collections.
2. Media assets are images or videos. Collection roots own lifecycle and folders; members inherit.
3. `All` means active accepted roots only. Inbox and Trash stay outside All and library scopes.
4. The durable ingest queue is the only entrypoint for manual, watch, subscription, and retry work.
5. Before 1.0, there are no migrations. A library must match the current schema or fail untouched.
6. Every mutation settles SQLite and projections before emitting one revision/resource invalidation.
7. PBIs close only after focused tests and an application-level smoke; completed PBIs are deleted.

## Phase 1: Backend Replacement

- Replace the layered backend with direct application operations and a current exact schema.
- Model library roots, media assets, physical files, collection membership, folders, tags, smart
  folders, source provenance, ingest work, and subscriptions explicitly.
- Make collection lifecycle and folder membership authoritative on the root; members inherit them.
- Implement group, detach, ungroup, reorder, cover selection, collection metadata fan-out, and
  destructive collection deletion.
- Use one canonical item query for grid, outline, selection, export, details, and counts.
- Keep projections incremental and rebuildable; never rebuild the whole library for ordinary writes.

## Phase 2: Communication Cutover

- Replace detailed state-change payloads with `revision`, affected resource keys, and item IDs.
- Resources are `library`, `sidebar`, `folders`, `smart_folders`, `tags`, `duplicates`,
  `subscriptions`, `settings`, `tasks`, and `item:{id}`.
- Frontend consumers re-query canonical data after committed invalidation. No speculative grid
  insertion or renderer-owned count patches remain.
- Verify drag-to-Inbox, drag-to-All, drag-to-Trash, folder moves, imports, and deletion settle in
  every open view without stale or ghost items.

## Phase 3: Subscriptions and Ingest

- Use one durable worker for scheduled and manual subscription runs, retry, stop, and restart.
- Persist source posts, source items, downloads, ingest state, retries, and terminal outcomes.
- Resume from non-terminal work; use source identity for idempotency and preserve deletion tombstones.
- Stream source items through the durable ingest queue. A multi-file post promotes its first item
  into a collection when the second item arrives, then appends later items in source order.
- Normalize all adapters to one post/item contract, sanitize descriptions centrally, and retain
  direct-site login through the OS credential store.
- Certify the supported source registry only after login, metadata, pagination, restart, and
  terminal-state behavior pass.

## Phase 4: Remaining Product Work

- Finish duplicates with deterministic quality comparison, metadata/provenance preservation, and
  safe collection-aware merge behavior.
- Finish tag management and durable automatic AI tagging.
- Finish deletion, recently viewed, folder/smart-folder behavior, and measured 100k-1M performance.
- Keep OnlyFans as a separate source runner using the same normalized subscription contract.
- Defer cloud sync, oplog, replay, conflict handling, and sync UI until after this release.

## Phase 5: Release Gate and Cleanup

- Delete replaced engine, DB façade, compiler/change-impact, renderer patch, and duplicate paths.
- Delete tests that only prove mocked forwarding; retain behavior and persistence tests.
- Remove commands, dependencies, documentation, and PBIs without active callers or release value.
- Run Rust formatting/tests, TypeScript/Vitest, command parity, native build, packaged Electron
  smoke, fresh-library schema checks, restart recovery, and representative performance probes.
- Delete this replacement PBI and other completed PBIs only after their focused smoke passes.

## Acceptance

- One production path exists for each user mutation, ingest path, query model, invalidation model,
  and subscription worker.
- Collections, All/Inbox/Trash, folders, smart folders, sidebar counts, grid counts, tags,
  duplicates, subscriptions, and restart recovery agree on persisted state.
- Cloud sync is absent from the release build and documentation.
- No pre-1.0 migration code exists.
