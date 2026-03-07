# PBI-240: Rust core full codebase audit for cleanup PBIs

## Priority
P1

## Audit Status (2026-03-06)
Status: **Partially Implemented**

Evidence:
1. Cleanup PBIs created so far (PBI-233 through PBI-239) were derived from targeted analysis of dispatch, tags, events, and files — not a comprehensive sweep.
2. Many modules have not been audited at all: `blob_store.rs`, `import_controller.rs`, `subscription_controller.rs`, `subscription_sync.rs`, `gallery_dl_runner.rs`, `ptr_client.rs`, `ptr_sync.rs`, `duplicates.rs`, `duplicate_controller.rs`, `smart_folder_controller.rs`, `flow_controller.rs`, `settings.rs`, `state.rs`, `perf.rs`, `media_protocol.rs`, `credential_store.rs`, and large parts of `sqlite/`.
3. Known patterns of drift: fixes applied without cleanup, redundant code paths left behind, inconsistent error handling, stale TODO comments.


## Audit Progress (2026-03-07)
This architecture-focused audit pass reviewed the full Rust core file inventory, deep-read the largest and most structurally important backend modules, and produced the following outputs:

1. `docs/rust-core-backend-rearchitecture-audit-2026-03-07.md`
2. `PBI-300` through `PBI-309`
3. Cross-link to the runtime communication design docs:
   - `docs/backend-frontend-state-rearchitecture.md`
   - `docs/pbi-234-runtime-communication-implementation-plan.md`

This does **not** mean every low-level bug has been catalogued. It does mean the major backend re-architecture tracks are now identified and split into concrete PBIs.

Additional backend-only artifacts produced after the first pass:

1. `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`
2. `docs/backend-legacy-register-2026-03-07.md`
3. `docs/backend-deletion-program-2026-03-07.md`
4. `PBI-315` through `PBI-320`

This means the audit now covers both:
- architecture/restructure tracks
- explicit backend legacy classification and deletion planning

## Recommended Execution Order (Backend)
The 300-series PBIs are not all peers. They have a hard dependency order if the goal is to stop re-encoding wrong behavior.

1. `PBI-327` canonical scope semantics engine
2. `PBI-330` business-logic contract audit and conformance tests
3. `PBI-328` fact-based mutation receipts and model-level invalidation
4. `PBI-329` derived resource dependency map from model facts
5. `PBI-300` runtime event bus and task registry realignment
6. `PBI-307` grid/selection/sidebar query service decomposition
7. `PBI-301` app state service lifecycle and worker boundary cleanup
8. `PBI-302` subscription domain service split and orchestration cleanup
9. `PBI-308` PTR domain decomposition and runtime state cleanup
10. `PBI-303` gallery-dl runner decomposition and site adapter split
11. `PBI-304` SQLite schema and migration pack decomposition
12. `PBI-305` derived read-model publish boundary cleanup
13. `PBI-306` import, lifecycle, and entity pipeline realignment
14. `PBI-309` media-processing adapter registry and pipeline breakup

Rationale:
1. business semantics must be canonical before runtime invalidation is formalized
2. mutation receipts must describe model truth before task/runtime infrastructure fans them out
3. query-service decomposition should consume canonical semantics, not invent them
4. backend topology and media-processing cleanup should follow after runtime/read-side contracts are stable

## Problem
The Rust core has accumulated technical debt that is only partially catalogued. The existing cleanup PBIs address known pain points, but a systematic sweep of every module is needed to surface:
- Dead or redundant code paths
- Inconsistent error handling patterns
- Stale workarounds that are no longer needed
- Missing or incorrect invariants
- Functions that have drifted from their intended purpose
- Code that "kind of works" but is fragile or incorrect

Without this audit, cleanup effort is reactive (fix what breaks) instead of proactive (fix what's wrong).

## Scope
- Every file in `core/src/` — read, understand, identify issues
- Every file in `core/src/sqlite/` — same
- Every file in `core/src/dispatch/` — same
- Every file in `core/src/files/` — same
- Output: a set of new PBIs (or additions to existing PBIs) for each issue found

## Implementation
1. Go through every module in `core/src/` systematically, one domain at a time.
2. For each module, document:
   - **What it's supposed to do** (inferred from code + any existing docs)
   - **What it actually does** (read the implementation)
   - **Gaps**: dead code, redundant paths, inconsistent patterns, missing error handling, stale TODOs
   - **Drift**: places where fixes were applied but the original issue wasn't cleaned up
3. For each issue found, either:
   - Create a new PBI if it's a distinct cleanup task
   - Append to an existing PBI if it fits an already-defined scope
4. Produce a summary audit report listing all modules reviewed and all PBIs created/updated.

## Modules to audit (checklist)

### Top-level
- [ ] `blob_store.rs`
- [ ] `constants.rs`
- [ ] `credential_store.rs`
- [ ] `duplicate_controller.rs`
- [ ] `duplicates.rs`
- [ ] `events.rs`
- [ ] `flow_controller.rs`
- [ ] `folder_controller.rs`
- [ ] `gallery_dl_runner.rs`
- [ ] `grid_controller.rs`
- [ ] `import.rs`
- [ ] `import_controller.rs`
- [ ] `lib.rs`
- [ ] `lifecycle_controller.rs`
- [ ] `media_protocol.rs`
- [ ] `metadata_controller.rs`
- [ ] `perf.rs`
- [ ] `poison.rs`
- [ ] `ptr_client.rs`
- [ ] `ptr_controller.rs`
- [ ] `ptr_sync.rs`
- [ ] `ptr_types.rs`
- [ ] `rate_limiter.rs`
- [ ] `selection_controller.rs`
- [ ] `selection_helpers.rs`
- [ ] `settings.rs`
- [ ] `sidebar_controller.rs`
- [ ] `smart_folder_controller.rs`
- [ ] `state.rs`
- [ ] `subscription_controller.rs`
- [ ] `subscription_sync.rs`
- [ ] `tag_controller.rs`
- [ ] `tags.rs`
- [ ] `types.rs`
- [ ] `view_prefs_controller.rs`

### sqlite/
- [ ] `bitmaps.rs`
- [ ] `collections.rs`
- [ ] `compilers.rs`
- [ ] `duplicates.rs`
- [ ] `files.rs`
- [ ] `flows.rs`
- [ ] `folders.rs`
- [ ] `hash_index.rs`
- [ ] `import.rs`
- [ ] `mod.rs`
- [ ] `projections.rs`
- [ ] `schema.rs`
- [ ] `sidebar.rs`
- [ ] `smart_folders.rs`
- [ ] `subscriptions.rs`
- [ ] `tags.rs`
- [ ] `view_prefs.rs`

### sqlite_ptr/
- [ ] `bootstrap.rs`
- [ ] `cache.rs`
- [ ] `mod.rs`
- [ ] `overlay.rs`
- [ ] `sync.rs`
- [ ] `tags.rs`

### dispatch/
- [ ] `common.rs`
- [ ] `duplicates.rs`
- [ ] `files.rs`
- [ ] `files_lifecycle.rs`
- [ ] `files_media.rs`
- [ ] `files_metadata.rs`
- [ ] `files_review.rs`
- [ ] `folders.rs`
- [ ] `grid.rs`
- [ ] `mod.rs`
- [ ] `ptr.rs`
- [ ] `selection.rs`
- [ ] `smart_folders.rs`
- [ ] `subscriptions.rs`
- [ ] `system.rs`
- [ ] `tags.rs`

### files/ (media processing)
- [ ] `archive.rs`
- [ ] `blurhash.rs`
- [ ] `colors.rs`
- [ ] `ffmpeg.rs`
- [ ] `ffmpeg_path.rs`
- [ ] `gallery_dl_path.rs`
- [ ] `image_metadata.rs`
- [ ] `mod.rs`
- [ ] `office.rs`
- [ ] `pdf.rs`
- [ ] `specialty.rs`
- [ ] `svg.rs`

## Acceptance Criteria
1. Every file in `core/src/` has been reviewed.
2. All identified issues are tracked as PBIs (new or appended to existing).
3. A summary audit report exists listing modules reviewed and findings.
4. No known technical debt is left uncatalogued.

## Test Cases
1. Audit report is complete — every module has an entry.
2. All new PBIs have clear scope and acceptance criteria.

## Risk
Low (it's an audit, not a code change). Time investment is medium-high (~1-2 days of focused review).
