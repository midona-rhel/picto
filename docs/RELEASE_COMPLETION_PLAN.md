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

Source certification may now resume. Existing source evidence remains historical until it is rerun
against independent media entities.

## Phase 3: PBI-575 Subscriptions

- [x] Freeze unrelated performance and cleanup work until subscription closure.
- [x] Replace newest-500 retry scans with one indexed attempt lookup.
- [x] S1: Give query and subscription issues stable non-null identity.
- [x] S2: Classify every meaningful failure through one persisted recovery disposition.
- [x] S3: Make each subscription the only scheduled run unit; make retry, restart, shutdown, stop,
      reset, delete, and multi-query finalization durable and safe.
- [x] S4: Finish one streaming metadata ingest path and delete proven competitors.
- [x] S5: Keep run-scoped progress visible and the run active through terminal ingest; make Health
      actions truthful and uncapped.
- [ ] S6: Certify sources in bounded batches, expose only passed sources, and run the real Electron
      workflow. Unattended tests may use an explicit test-only plaintext fixture through a
      process-local credential store, but must never read or modify the system credential store.

Prior acceptance on 2026-08-07 covered the then-current import, progress, stop/resume, and settled
state behavior. Its media-shape claims are obsolete. No source certification closes until the
flattened multi-file behavior is accepted.

Source closure starts with the 18-source production registry. Additional paid sources are added only
when their dedicated implementation enters that registry; an unimplemented source is not counted as
certified:

- [x] Audit every hidden registry entry against its packaged gallery-dl extractor and live probe.
- [x] Delete the shallow download-only verifier; one strict production-path certification now owns
      download, ingest, metadata, restart, resume, and archive replay.
- [ ] Recertify every source against the flattened media model. Collection-era artifacts are
      historical diagnostics and do not close current acceptance.
- [ ] Restore and finish the remaining 14 gallery-dl-backed sources through explicit source-family
      adapters. Do not use a generic fallback adapter to claim support. Historical evidence exists
      for several sources, but all current certification must prove independent media entities,
      source provenance, restart, and the Electron workflow after the flattened-model rewrite.
      SubscribeStar is gallery-dl-backed and remains in the paid-source certification matrix.
      Authenticated restricted-content access remains unfinished for sources that provide it.
- [ ] Add OnlyFans through a dedicated runner, not gallery-dl. It owns OnlyFans authentication,
      pagination, media resolution, and video downloads, then emits the same normalized source events
      and enters the same durable ingest queue as other sources.
- [ ] Give every source one direct-site login path: open the real source page in a Picto-managed
      browser, capture the resulting session, and store it in the OS credential store. Remove UI
      that asks users to paste passwords, cookies, tokens, or API keys.
- [ ] Certify every active registry source with the strict production-path harness and one real
      Electron workflow before S6 closes. A source remains hidden until it passes.

The authoritative source list and per-source acceptance state live in the active PBI-575 matrix.

Phase gates:

- Runtime gate after S3: stop/restart/retry manually verified; shutdown leaves no gallery-dl process,
  executor, lease, or false active run; subscription schedules pass interval, pause, full-run, and
  manual-query tests.
- Delivery gate after S5: streaming, independent multi-file media, metadata, and progress verified
  against the flattened model.
- Release gate after S6: every visible source has deterministic and live proof, credential-backed
  where that source supports credentials.

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
