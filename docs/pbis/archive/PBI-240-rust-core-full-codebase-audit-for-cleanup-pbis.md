# PBI-240: Rust core full codebase audit for cleanup PBIs

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Cleanup PBIs created so far (PBI-233 through PBI-239) were derived from targeted analysis of dispatch, tags, events, and files — not a comprehensive sweep.
2. Many modules have not been audited at all: `blob_store.rs`, `import_controller.rs`, `subscription_controller.rs`, `subscription_sync.rs`, `gallery_dl_runner.rs`, `ptr_client.rs`, `ptr_sync.rs`, `duplicates.rs`, `duplicate_controller.rs`, `smart_folder_controller.rs`, `flow_controller.rs`, `settings.rs`, `state.rs`, `perf.rs`, `media_protocol.rs`, `credential_store.rs`, and large parts of `sqlite/`.
3. Known patterns of drift: fixes applied without cleanup, redundant code paths left behind, inconsistent error handling, stale TODO comments.

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
