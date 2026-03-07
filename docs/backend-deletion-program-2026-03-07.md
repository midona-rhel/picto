# Backend Deletion Program

Date: 2026-03-07
Status: execution plan
Scope: `/Users/midona/Code/imaginator/core/src/**`

## Goal

Delete backend code aggressively and safely instead of carrying legacy forward into
the re-architecture.

This is not a cleanup wish list. It is the deletion-side execution plan that sits
next to the structural blueprint.

## Deletion principles

1. No backend restructure PR should be move-only unless there is a hard compiler constraint.
2. If a new canonical owner is introduced, the old compatibility owner must get a removal condition immediately.
3. Transitional code must be marked and scheduled for deletion; otherwise it is just new legacy.
4. The backend root should trend toward fewer files every cycle, not more.

## Program targets

### Phase 1: Immediate safe deletions

Target: `~350-450 LOC`

Delete or inline once call sites are moved:
- `core/src/sidebar_controller.rs`
- `core/src/import_controller.rs`
- `core/src/view_prefs_controller.rs`
- `core/src/lifecycle_controller.rs`
- `core/src/dispatch/files_review.rs`

Rationale:
These files are mostly wrappers or isolated leftovers. They add names and layering
without adding real ownership.

### Phase 2: Runtime compatibility purge

Target: `~600-1200 LOC`

Delete after runtime event bus / snapshot cutover:
- legacy `Domain` / `Invalidate` / `MutationImpact` sections in `core/src/events.rs`
- legacy event-name families superseded by runtime receipts/tasks
- duplicate task progress caches once `runtime_state` is authoritative

Rationale:
The current code is paying twice for the same runtime communication concern.

### Phase 3: Root controller collapse

Target: `~1500-3000 LOC`

Delete root controller homes after domain folderization:
- `folder_controller.rs`
- `tag_controller.rs`
- `smart_folder_controller.rs`
- `selection_controller.rs`
- `metadata_controller.rs`
- parts of `selection_helpers.rs`
- root aliases in `lib.rs`

Rationale:
These are not bad behaviors; they are bad homes. Once the domain folders exist,
keeping these root files would be pure legacy.

### Phase 4: Monolith breakup and tail deletion

Target: `~4000-9000 LOC`

Delete monolithic shells after splits land:
- `gallery_dl_runner.rs`
- `subscription_controller.rs`
- `subscription_sync.rs`
- `grid_controller.rs`
- `files/mod.rs`
- `sqlite/schema.rs`
- `sqlite/mod.rs`
- `sqlite_ptr/mod.rs`
- `runtime_state.rs`
- `runtime_contract/mod.rs`

Rationale:
These files should not survive as giant single ownership units if the backend
re-architecture is real.

## Guardrails after each phase

1. Add CI check blocking new unapproved root files.
2. Add CI check blocking imports from removed legacy paths.
3. Maintain a `LEGACY:` marker convention for temporary shims.
4. Require every backend PBI touching structure to include a deletion section.

## Required reporting per backend restructuring PR

Every PR should state:
1. Which canonical owner gained the logic.
2. Which old path became legacy.
3. How many lines were deleted.
4. Whether any temporary compatibility path remains.
5. What future PBI removes that compatibility path.

## Backend-only deletion PBIs to execute

1. `PBI-315`: backend legacy register and ownership classification pass
2. `PBI-316`: immediate low-risk backend wrapper and review-path deletions
3. `PBI-317`: runtime event-system compatibility purge after mutation/task cutover
4. `PBI-318`: root controller collapse and alias purge
5. `PBI-319`: monolith tail breakup and post-split deletion campaign
6. `PBI-320`: backend topology enforcement and CI guardrails

## Success criteria

1. Backend root is reduced to approved top-level modules only.
2. No wrapper controller files remain at root.
3. No legacy invalidation/event model remains after runtime cutover.
4. No backend monolith survives solely because nobody scheduled its deletion.
5. The backend has an explicit, enforced rule set that prevents legacy regrowth.
