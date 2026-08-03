# Picto Release Completion Plan

## Goal

Finish the application without another architecture program. Keep one production path per behavior,
prove persistence and user-visible outcomes, delete replaced code, and ship a packaged Electron build.

## Release Rules

1. SQLite is truth; Roaring bitmaps are rebuildable query projections.
2. `add_media` is the only ingest entrypoint for manual imports, watches, subscriptions, and retries.
3. Collections contain images only and aggregate child metadata.
4. Durable background work owns derivatives and automatic AI tagging.
5. Folder sync uses a user-owned directory transported by Drive, Dropbox, or equivalent software.
6. Every visible action works or is removed.
7. PBIs close only after focused tests and an application smoke.
8. Agents never share write scopes or commit independently.

## Phase 0: Clean Integration

- [x] Resolve all unmerged paths without dropping current behavior.
- [x] Delete the conflicted legacy frontend rather than resolving obsolete behavior.
- [x] Require an empty unmerged-file list and clean `git diff --check`.
- [x] Run Rust tests, TypeScript, Vitest, and command parity.
- [x] Create one reviewed integration baseline commit.
- [x] Delete the remaining unreachable `legacy/` tree after reachability and packaging checks.

## Phase 1: Truthful Verification and Migrations

- [x] Remove dead runtime-schema, typed-command, and undo-coverage checks.
- [x] Keep TypeScript, Vitest, Rust tests, command parity, and behavior-boundary tests.
- [x] Replace the stale smoke script with a real packaged-app launch/open-library smoke.
- [x] Create only the current pre-1.0 schema and validate exact-version opens.
- [x] Reject old, newer, malformed, and unknown schemas without mutating user data.
- [x] Delete legacy imports, ordered migrations, and canonical open-time schema mutation.

## Phase 2: Core Library Contracts

- [x] Prove manual, collection, folder, watch, subscription, and retry imports use the durable queue.
- [x] Delete competing import/retry paths; keep explicit user-requested derivative repair actions.
- [x] Reject videos from collection creation, queued imports, and membership before writes.
- [x] Create collections with members atomically; keep split non-destructive and deletion destructive.
- [ ] Verify collection metadata aggregation and tag fan-out.
- [x] Delete folders atomically with all descendants while leaving media untouched.
- [x] Fall back to the nearest surviving parent or All when deleting the active folder hierarchy.
- [x] Emit every deleted folder in runtime facts.
- [ ] Verify grid, sidebar, folder, smart-folder, tag, and special-scope counts agree.
- [ ] Verify recently viewed is unique per entity and ordered by latest view.
- [x] Remove incomplete batch rename and no-op folder actions; retain drag-and-drop folder Move.

## Phase 3: PBI-575 Subscriptions

- [ ] Replace newest-500 retry scans with one indexed attempt lookup.
- [ ] Give issues stable identity independent of mutable message text.
- [ ] Deduplicate subscription-wide issues with null query IDs.
- [ ] Persist blocked, retryable, reviewable, and terminal recovery semantics.
- [ ] Make the Health surface explain and expose the recovery action.
- [ ] Consolidate repeated runtime-service construction only where it removes glue.
- [ ] Test old retries, repeated issues, restart recovery, failures, and canonical ingest.
- [ ] Run a real Electron create/run/fail/retry/import smoke and archive PBI-575.

## Phase 4: PBI-577 Duplicates

- [ ] Require an existing unresolved candidate before destructive resolution.
- [ ] Make failed loser-blob cleanup visible and repairable.
- [ ] Test every decision and cross-collection ownership choice.
- [ ] Verify reference repointing and blob state after restart.
- [ ] Measure the simple scan on a representative library before optimizing it.
- [ ] Delete unused similar-media code if no release surface calls it.
- [ ] Run the Electron duplicate-review smoke and archive PBI-577.

## Phase 5: PBI-604 Tag Manager

- [ ] Return tag pages as `{ items, next_cursor }`.
- [ ] Build one direct rebuilt Tag Manager without a manager framework or legacy port.
- [ ] Support search, namespace filtering, stable pagination, and zero-count tags.
- [ ] Support rename, merge, delete, aliases, implications, and site masks.
- [ ] Settle grid, inspector, smart-folder, untagged, and sidebar reads through normal facts.
- [ ] Test real mutations, remove the placeholder, run the Electron smoke, and archive PBI-604.

## Phase 6: PBI-605 AI Tagging

- [ ] Download model artifacts into a temporary directory and activate them atomically.
- [ ] Validate labels and load the ONNX session before reporting ready.
- [ ] Prove preprocessing, channel order, normalization, thresholds, and output interpretation.
- [ ] Use one prediction helper for reviewed and automatic tagging.
- [ ] Move auto-tagging from ingest into durable retryable background work.
- [ ] Keep reviewed application explicit and preserve AI provenance.
- [ ] Make cancellation behavior honest and leave no stuck task.
- [ ] Run restart and packaged CPU inference smokes and archive PBI-605.

## Phase 7: PBI-602 Folder Sync

- [ ] Replace the architecture document with a short folder-sync behavior contract.
- [ ] Use one `sync_cycle` for startup, periodic, and manual sync.
- [ ] Upload verified blobs before operations that reference them.
- [ ] Verify downloaded content hashes before storing originals.
- [ ] Reject symlinks and paths escaping the selected sync root.
- [ ] Park missing-prerequisite operations instead of advancing past them.
- [ ] Stop safely on unknown operation versions or types.
- [ ] Track and retry missing blobs.
- [ ] Enqueue derivatives exactly once after remote blob hydration.
- [ ] Persist last success, failure, pending work, and missing-media state.
- [ ] Sync subscription definitions and groups; keep credentials and run history device-local.
- [ ] Preserve tag provenance during replay.
- [ ] Report uploads, downloads, pending work, failures, and derivative catch-up truthfully.
- [ ] Pass two-device restore, corruption, ordering, and restart tests, then archive PBI-602.

## Phase 8: Deletion and PBI Cleanup

- [x] Delete unused frontend dependencies and update third-party licenses.
- [ ] Remove every production TODO by implementing, removing, or documenting a real limitation.
- [ ] Remove unsupported authentication entrypoints.
- [ ] Delete commands without active callers and minimize the parity allowlist.
- [ ] Consolidate only measured duplicate behavior, not files that are merely large.
- [ ] Delete archived PBIs, historical plans, and stale architecture documents after closure.
- [ ] Replace the large guide program with concise release-accurate user documentation.
- [x] Delete unreproduced bug buckets, absent Random work, and legacy-parity menu work.
- [ ] Make first launch lead directly to creating a library, then archive PBI-227.

## Phase 9: Release Gate

- [ ] Clean Git index and diff checks.
- [ ] Rust formatting and full Rust tests.
- [ ] TypeScript, Vitest, and command parity.
- [ ] Fresh, exact-version, malformed, and mismatch schema tests.
- [ ] Native module build and packaged Electron build.
- [ ] Smoke import, grid, inspector, folders, collections, subscriptions, duplicates, tags, AI,
      sync, deletion, and restart recovery.
- [ ] Run supported platform checks and archive PBI-603.

## Agent Waves

1. Integration is coordinator-owned; one read-only agent reviews conflict choices.
2. Migrations, verification scripts, and docs use separate Terra agents with disjoint files.
3. Subscription and duplicate correctness may run in parallel; shared schema edits are integrated
   serially.
4. Tag backend lands before the Tag Manager UI agent begins.
5. AI tagging runs mostly alone because it crosses downloads, inference, workers, ingest, and UI.
6. Sync backend work runs alone in ordered slices; its UI starts only after the status contract is stable.
7. Deletion, docs, and final smoke work run in parallel only when their write scopes do not overlap.

Every agent prompt defines behavior, allowed files, forbidden refactors, deletions, and focused tests.
The coordinator reviews the diff, runs the gate, and commits one coherent slice at a time.
