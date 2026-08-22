# Picto Release Completion Plan

## Goal

Finish the application without another architecture program. Keep one production path per behavior,
prove persistence and user-visible outcomes, delete replaced code, and ship a packaged Electron build.

## Release Rules

1. SQLite is truth; Roaring bitmaps are rebuildable query projections.
2. The durable ingest queue is the only ingest entrypoint for manual imports, watches,
   subscriptions, and retries.
3. Media entities are images or videos only. Multi-file posts create independent media with shared
   source-post metadata and order; no hidden grouping or automatic folders are created.
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

- [x] Prove manual, folder, watch, subscription, and retry imports use the durable queue.
- [x] Delete competing import/retry paths; keep explicit user-requested derivative repair actions.
- [x] Keep image and video media separate from explicit folder membership and source metadata.
- [x] Delete folders atomically with all descendants while leaving media untouched.
- [x] Fall back to the nearest surviving parent or All when deleting the active folder hierarchy.
- [x] Emit every deleted folder in runtime facts.
- [x] Verify grid, sidebar, folder, smart-folder, tag, and special-scope counts agree.
- [x] Verify recently viewed is unique per entity and ordered by latest view.
- [x] Remove incomplete batch rename and no-op folder actions; retain drag-and-drop folder Move.
- [x] Make manual file drag-and-drop honor the active lifecycle destination. Inbox imports remain
      Inbox media, and every open grid settles membership from its canonical query after import.
      Verify `All` remains unchanged, Inbox increases, and the same durable ingest path is
      used rather than adding a second import implementation.

## Phase 2.5: Flattened Media Model

- [x] Media entities are images or videos only; the aggregate-entity path is deleted.
- [x] Import every file in a multi-file source post as an independent media entity with shared
      source-post metadata and source order.
- [x] Never create a hidden group, placeholder, automatic per-post folder, or grouping extension hook.
- [x] Keep `All` as the accepted active library and exclude Inbox/Trash from `All`, folders, smart
      folders, search, and counts.
- [x] Allow any future grouping or rearrangement only through a dedicated external media
      manifest/file format, outside the current media data model.
- [x] Delete the replaced aggregate path and pass compile, unit, contract, parity, and build tests.
- [x] Pass the fresh-library packaged-application smoke and delete the completed implementation PBI.

Source certification may now resume. Collection-shaped persistence assertions are obsolete, but
accepted authentication, extraction, metadata, pagination, recovery, and UI evidence remains valid
unless the code that owns that behavior changed. The independent-media ingest layer is proved once
with representative single-file, ordered multi-file, and mixed-media sources rather than by
re-downloading every source.

## Phase 3: Subscriptions

- [x] Give query and subscription issues stable identity and one persisted recovery disposition.
- [x] Make the subscription the only scheduled run unit; make restart, stop, retry, reset, delete,
      and multi-query completion durable.
- [x] Stream every source through the durable ingest queue and keep progress active until downloads
      and ingest are terminal.
- [x] Use explicit source adapters rather than a generic fallback, and certify extraction, metadata,
      pagination, interruption, restart, replay, and user-visible terminal state.
- [x] Use direct-site login in a Picto-managed browser and store captured credentials in the OS
      credential store. Product UI never asks users to paste secrets.
- [ ] Certify the current production registry: Pixiv search, Pixiv user, Gelbooru, Rule34,
      Danbooru, Webtoons, Hentai Foundry, Baraag, DeviantArt, Tumblr, Fur Affinity, Patreon,
      pixivFANBOX, SubscribeStar, Idol Complex, Sankaku, Yande.re, Konachan, Safebooru, and e621.
- [x] Prove the shared flattened ingest model with representative single-file, ordered multi-file,
      and mixed image/video sources instead of repeating unchanged 100-post downloads per adapter.
- [x] Run real Electron login and ingest workflows, including Webtoons cookie capture and Tumblr
      OAuth credential capture.
- [x] Remove deferred ArtStation from the registry and Accounts UI rather than advertising an
      uncertified source.

Baraag's public production path is certified. Its optional private login cannot be certified with a
throwaway account because the source requires an applicant's own artwork and moderator approval;
Picto retains the direct OAuth path without pretending that external approval occurred. Patreon,
pixivFANBOX, and SubscribeStar now use the same direct-site cookie-capture contract as the other
cookie-auth sources and remain pending attended certification. OnlyFans remains outside the current
registry because it needs a separate downloader/runtime path for mixed image and video handling.

## Phase 4: PBI-577 Duplicates

- [x] Preserve the existing-candidate guard in the release test lane before every destructive
      resolution.
- [x] Make failed loser original/thumbnail blob cleanup visible and durably retryable.
- [x] Test every decision and cross-media ownership choice against a live candidate.
- [x] Verify reference repointing, All/Inbox/Trash boundaries, and blob state after restart.
- [x] Measure and replace the unconditional quadratic scan. The exact indexed implementation
      matched brute force on 4,096 deterministic 256-bit hashes and all tested thresholds; the
      release measurement was 13.57 ms brute force and 20.00 ms indexed at this small population,
      with no unsupported large-library speed claim.
- [x] Replace per-import full-library pHash scans with a durable eight-partition SQLite index for
      the normal 97% threshold. Candidate lookup and pair insertion now share the import write
      transaction; a one-million-row query-plan probe used all eight indexes and returned one
      synthetic candidate in about 1 ms.
- [x] Delete unused similar-media commands, types, BK-tree path, and parity exception.
- [x] Run the packaged Electron duplicate-review smoke through rendered controls. Evidence:
      `artifacts/duplicates/smoke.json` on 2026-08-14.
- [x] Archived PBI-577 after focused, full-suite, package, restart, and UI-smoke verification.

## Phase 5: PBI-604 Tag Manager

- [x] Return tag pages as `{ items, next_cursor }`.
- [x] Build one direct rebuilt Tag Manager without a manager framework or legacy port.
- [x] Support search, namespace filtering, stable pagination, and zero-count tags.
- [x] Support rename, merge, delete, aliases, and implications.
- [x] Settle grid, inspector, smart-folder, untagged, and sidebar reads through normal facts.
- [x] Test real mutations, remove the placeholder, run the Electron smoke, and archive PBI-604.

Accepted 2026-08-14. The behavior is release-complete; visual restyling remains part of the later
reference application-reference UI pass and must reuse this canonical API rather than introducing another tag path.

## Phase 6: PBI-605 AI Tagging

- [x] Download model artifacts into a temporary directory and activate them atomically.
- [x] Validate labels and load the ONNX session before reporting ready.
- [x] Prove preprocessing, channel order, normalization, thresholds, and output interpretation.
- [x] Use one prediction helper for reviewed and automatic tagging.
- [x] Move auto-tagging from ingest into durable retryable background work.
- [x] Keep reviewed application explicit and preserve AI provenance.
- [x] Make cancellation behavior honest and leave no stuck task.
- [ ] Run restart and packaged CPU inference smokes and archive PBI-605.

## Phase 7: PBI-602 Folder Sync

- [x] Replace the architecture document with a short folder-sync behavior contract.
- [x] Use one `sync_cycle` for startup, periodic, and manual sync.
- [x] Upload verified blobs before operations that reference them.
- [x] Verify downloaded content hashes before storing originals.
- [x] Reject symlinks and paths escaping the selected sync root.
- [x] Park missing-prerequisite operations instead of advancing past them.
- [x] Stop safely on unknown operation versions or types.
- [x] Track and retry missing blobs.
- [x] Enqueue derivatives exactly once after remote blob hydration.
- [x] Persist last success, failure, pending work, and missing-media state.
- [x] Sync subscription definitions and queries; keep credentials and run history device-local.
- [x] Preserve tag provenance during replay.
- [x] Report uploads, downloads, pending work, failures, and derivative catch-up truthfully.
- [x] Pass two-device restore, corruption, ordering, and restart tests, then archive PBI-602.

## Phase 8: Performance, Deletion, and PBI Cleanup

- [x] Delete unused frontend dependencies and update third-party licenses.
- [ ] Benchmark single writes, bulk writes, startup rebuilds, and large smart-folder reads on
      representative 100k-1M entity data.
      Baseline: one status change at 100k entities took 1.7s with 10 smart folders and 20.7s with
      100 smart folders; projection rebuilding, not the sub-millisecond SQL write, dominated.
- [ ] Keep common status and tag writes incremental; they must not trigger full-library tag,
      smart-folder, sidebar, or cached-size rebuilds.
- [ ] Make query-result bulk mutations ID-only and set-based, with bounded runtime events.
- [ ] Stop expanding large smart-folder bitmaps into SQL `IN` lists.
- [ ] Remove every production TODO by implementing, removing, or documenting a real limitation.
- [ ] Remove unsupported authentication entrypoints.
- [ ] Delete commands without active callers and minimize the parity allowlist.
- [ ] Route actionable operation failures and background completion summaries through one shared
      non-modal notification path; raw IPC wrapper errors and local feature banners must not leak
      into the UI.
- [ ] Consolidate only measured duplicate behavior, not files that are merely large.
- [ ] Delete archived PBIs, historical plans, and stale architecture documents after closure.
- [ ] Replace the large guide program with concise release-accurate user documentation.
- [x] Delete unreproduced bug buckets, absent Random work, and legacy-parity menu work.
- [x] Remove guided onboarding as a feature. Cold start keeps one ordinary create/open-library state
      and does not mount library-scoped UI or issue library-scoped backend calls before open succeeds.

## Phase 9: Release Gate

- [ ] Audit the release test harness by behavior: delete tests that only prove mocked values move
      between preconfigured layers, retain focused unit tests without calling them product proof,
      and require real persistence/application evidence for every release claim.
- [ ] Clean Git index and diff checks.
- [ ] Rust formatting and full Rust tests.
- [ ] TypeScript, Vitest, and command parity.
- [ ] Fresh, exact-version, malformed, and mismatch schema tests.
- [ ] Native module build and packaged Electron build.
- [ ] Smoke import, grid, inspector, folders, flattened multi-file subscription imports, duplicates,
      tags, AI, sync, deletion, and restart recovery.
- [ ] Run supported platform checks and archive PBI-603.
- [ ] Clean Rust targets and stale packaged output after each source-certification batch and before
      the final packaged build; do not let repeated native test links accumulate indefinitely.

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
