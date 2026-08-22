# Picto Release Completion Plan

## Goal

Ship one understandable backend: command, application operation, SQLite transaction, synchronous
projection settlement, compact resource invalidation. Delete each replaced path in the same cutover.

## Release Rules

- `All` is active roots only. Inbox and Trash are separate.
- Library roots are standalone media or collections. Attached members have no independent lifecycle
  or folder membership.
- One ingest queue accepts manual, watched, and subscription media.
- One persisted subscription state machine owns runs, retries, interruption, and progress.
- SQLite is authoritative; projections are settled before readers and events observe a revision.
- No pre-1.0 migration, dual writes, compatibility backend, cloud sync, or disabled release UI.
- Preserve the current active test library until cutover. Then run one reviewed manual conversion,
  verify the converted copy, and delete the conversion tool before release.
- Retained tests prove behavior or persistence. Wrapper/parity/mock-only tests are deleted.

## Phase 0: Integration Boundary

- [x] Commit the prior subscription/frontend checkpoint.
- [x] Isolate the replacement in `codex/backend-replacement`.
- [x] Restore the collection/root product contract and remove cloud sync from release scope.
- [ ] Keep one replacement PBI and delete it after the packaged smoke.

## Phase 1: Store and Application Core

- [x] Create exact schema 118 and reject incompatible databases without mutation.
- [x] Separate item, media, and physical file identity.
- [x] Add direct Store read/transaction boundaries and monotonic library revision.
- [x] Add compact mutation receipts and `LibraryChanged` resources.
- [ ] Settle projection changes under the same read/write consistency boundary as SQLite commits.

## Phase 2: Queries, Projections, and Core Operations

- [x] Implement canonical All, Inbox, Trash, Recently Viewed, Untagged, Uncategorized, and folder
  root queries with visible-item and underlying-media counts.
- [ ] Implement smart-folder and search predicates through the same root resolver.
- [ ] Use one query for pages, outline, selection, export, and sidebar counts.
- [x] Implement lifecycle, folders, tags, metadata, group, detach, ungroup, reorder, cover, and
  destructive delete as transaction-owned operations.
- [ ] Prove incremental projection behavior at 100k and 1M representative assets.

## Phase 3: Ingest and Background Work

- [x] Implement physical-byte reuse with distinct logical media occurrences.
- [x] Implement source-item idempotency, deletion tombstones, and second-item collection promotion.
- [ ] Make the durable ingest queue the only manual/watch/subscription entrypoint.
- [ ] Finish one durable worker for derivatives, AI tagging, and blob deletion.

## Phase 4: Subscriptions

- [ ] Replace transient run ownership with one persisted subscription/query-run state machine.
- [ ] Resume from the first non-terminal source item after restart.
- [ ] Derive all counters from persisted rows and serialize same-domain requests one second apart.
- [ ] Normalize every adapter to ordered posts/items and sanitize descriptions centrally.
- [ ] Reuse unchanged authentication/extraction evidence and recertify changed adapters.

## Phase 5: Duplicates and AI

- [ ] Detect at physical-file level and present affected logical roots.
- [ ] Resolve deterministic quality winners using decoded information, dimensions, format, alpha,
  file size, and similarity.
- [ ] Repoint logical occurrences without losing source provenance, ordering, folders, or tags.
- [ ] Finish automatic tagging through the shared durable worker.

## Phase 6: Atomic App Cutover

- [ ] Build a one-time development conversion for the current active library; dry-run and back it up
  before mutation. Never ship or auto-run this conversion.
- [ ] Replace hash-based logical UI identity with item IDs.
- [ ] Replace detailed state-change settlement with the resource invalidation registry.
- [ ] Remove optimistic grid/count ownership and reconcile from canonical queries.
- [ ] Remove unused commands and switch IPC/backend/frontend contracts atomically.
- [ ] Delete old engine, DB facade, compiler/change-impact, old ingest/subscription paths, and cloud
  sync immediately after the smoke passes.
- [ ] Delete the one-time conversion tool after the converted active library passes verification.

## Phase 7: OnlyFans

- [ ] Add a native source runner behind the same normalized post/item stream.
- [ ] Prove direct-site auth, images, videos, mixed posts, pagination, restart, and expired sessions.

## Phase 8: Release Gate

- [ ] Remove production TODO/FIXME items by implementation or deletion.
- [ ] Delete mock-only/pass-through tests and obsolete PBIs/docs.
- [ ] Pass Rust, TypeScript, Vitest, native addon, packaged Electron, and behavior smokes.
- [ ] Report production/test LOC and deleted modules.
- [ ] Delete the replacement PBI.

## User Verification

1. Core: import, lifecycle drag, folders, group/reorder/detach/ungroup, destructive collection delete.
2. Subscriptions: booru and multi-media creator run, interruption, restart, truthful progress/counts.
3. Cutover: grids, sidebar, inspector, duplicates, tags, and subscriptions settle without refresh or
   visual ghost items.
4. OnlyFans: attended login and representative image/video/mixed posts.
5. Packaged release: fresh library through import, acceptance, organization, subscription,
   deduplication, tagging, AI tagging, restart, and deletion.
