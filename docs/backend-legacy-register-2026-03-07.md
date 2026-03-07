# Backend Legacy Register

Date: 2026-03-07
Status: active audit artifact
Scope: `/Users/midona/Code/imaginator/core/src/**`
Source of truth: current backend tree, including the in-flight runtime communication migration

## Purpose

This register is the backend-only classification pass that the earlier audit was
missing.

It answers four concrete questions for every backend file group:

1. Is this code canonical, transitional, merge-legacy, or delete-legacy?
2. Which target module should own it after the re-architecture?
3. What is the removal condition for the legacy path?
4. Which files are realistic early deletion targets versus later merge-delete targets?

## Status labels

- `canonical`: keep, but possibly move into the target tree
- `transitional`: current in-flight direction; do not delete yet
- `legacy-merge`: behavior still matters, but the file should disappear by being folded into a domain/runtime/persistence module
- `legacy-delete`: remove once the replacement path exists or, if replacement already exists, remove in the next deletion pass

## Immediate conclusions

1. The new `runtime_contract/*` and `runtime_state.rs` are the correct direction and should be treated as transitional-canonical.
2. The biggest legacy concentration is still in the flat root: controller wrappers, event compatibility, and leftover one-off dispatch modules.
3. The backend is currently carrying both:
   - a new runtime contract and task registry
   - the old invalidation/event model (`Domain`, `Invalidate`, `MutationImpact`, legacy event names)
   This is the single clearest merge-delete seam.
4. There is an immediate low-risk deletion budget in thin wrapper modules.
5. There is a second, much larger merge-delete budget in root controllers, duplicated orchestration, and compatibility event paths.

## Estimated deletion budget

- Immediate safe delete / fold-away budget: `~600-1500 LOC`
- Medium merge-then-delete budget: `~3500-7000 LOC`
- Full backend deletion program after re-architecture cutovers: `~12000-18000 LOC`

The upper range is realistic because many root modules are not individually dead, but
are structurally duplicate homes for behavior that should become domain-owned.

## Classification by file group

### Root modules

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/blob_store.rs` | canonical | `infra/blob_store.rs` | move | Stable infrastructure module. |
| `core/src/constants.rs` | canonical | `infra/constants.rs` | move/split | Large shared constants module; keep but prune dead constants during split. |
| `core/src/credential_store.rs` | canonical | `infra/credential_store.rs` | move | Correct responsibility, wrong location. |
| `core/src/duplicate_controller.rs` | legacy-merge | `domains/duplicates/controller.rs` | merge-delete | Root controller shell; duplicate home next to `duplicates.rs` and `sqlite/duplicates.rs`. |
| `core/src/duplicates.rs` | canonical | `domains/duplicates/matching.rs` | move | Core matching/BK-tree logic is real; file should survive as domain internals. |
| `core/src/events.rs` | transitional | `runtime/events.rs` + `runtime/receipts.rs` | split then delete legacy sections | Keep new runtime emit path; delete `Domain`, `Invalidate`, `MutationImpact`, and legacy event families after cutover. |
| `core/src/flow_controller.rs` | legacy-merge | `domains/flows/controller.rs` + `domains/flows/orchestrator.rs` | merge-delete | Heavy orchestration in wrong home. |
| `core/src/folder_controller.rs` | legacy-merge | `domains/folders/controller.rs` + `domains/folders/membership.rs` | merge-delete | Real behavior, wrong top-level ownership. |
| `core/src/gallery_dl_runner.rs` | legacy-merge | `domains/subscriptions/gallery_dl/*` | split-delete | Monolith is too large; final file should not survive intact. |
| `core/src/grid_controller.rs` | legacy-merge | `domains/grid/controller.rs` + `domains/grid/query.rs` | split-delete | Major read-path module; canonical behavior but wrong structure. |
| `core/src/import.rs` | canonical | `domains/files/ingest.rs` | move | Real import pipeline implementation. |
| `core/src/import_controller.rs` | legacy-delete | `domains/files/controller.rs` / `domains/files/ingest.rs` | delete after call-site move | Thin wrapper only. |
| `core/src/lifecycle_controller.rs` | legacy-delete | `domains/files/lifecycle.rs` | delete after call-site move | Tiny wrapper over DB/blob operations. |
| `core/src/media_protocol.rs` | canonical | `infra/media_protocol.rs` | move | Correct infra concern. |
| `core/src/metadata_controller.rs` | legacy-merge | `domains/files/metadata.rs` | merge-delete | Real metadata assembly, but should not remain root-level. |
| `core/src/perf.rs` | canonical | `infra/perf.rs` | move | Keep, but unify perf sinks/labels as part of migration. |
| `core/src/poison.rs` | canonical | `infra/poison.rs` | move | Small shared infrastructure. |
| `core/src/ptr_client.rs` | legacy-merge | `domains/ptr/client.rs` | move/delete root | Real behavior, wrong home. |
| `core/src/ptr_controller.rs` | legacy-merge | `domains/ptr/controller.rs` | merge-delete | Domain controller at wrong layer. |
| `core/src/ptr_sync.rs` | legacy-merge | `domains/ptr/sync.rs` | merge-delete | Sync pipeline belongs inside PTR domain. |
| `core/src/ptr_types.rs` | legacy-merge | `domains/ptr/types.rs` | move/delete root | Type home should be PTR domain or shared contract. |
| `core/src/rate_limiter.rs` | canonical | `infra/rate_limiter.rs` | move | Correct infrastructure module. |
| `core/src/runtime_state.rs` | transitional | `runtime/task_registry.rs` + `runtime/snapshot.rs` + `runtime/sequence.rs` | split/move | Keep as source of truth during migration; final file should disappear into runtime folder. |
| `core/src/selection_controller.rs` | legacy-merge | `domains/selection/controller.rs` + `domains/selection/query.rs` | merge-delete | Real behavior, wrong home. |
| `core/src/selection_helpers.rs` | legacy-merge | `domains/selection/summary.rs` + `domains/grid/scopes.rs` | split-delete | Mixed helper file spanning selection and grid concerns. |
| `core/src/settings.rs` | legacy-merge | `domains/settings/store.rs` | merge-delete | Runtime store/config concerns should not remain root-level. |
| `core/src/sidebar_controller.rs` | legacy-delete | `domains/folders/*`, `domains/smart_folders/*`, `persistence/publish/sidebar.rs` | delete after call-site move | Extremely thin wrapper; pure compatibility shell. |
| `core/src/smart_folder_controller.rs` | legacy-merge | `domains/smart_folders/controller.rs` + `domains/smart_folders/predicates.rs` | merge-delete | Real behavior, wrong top-level ownership. |
| `core/src/state.rs` | transitional | `app/library_runtime.rs` + `app/worker_runtime.rs` + `app/startup.rs` | split-delete | Central runtime composition file; keep during migration, but final monolith must disappear. |
| `core/src/subscription_controller.rs` | transitional | `domains/subscriptions/controller.rs` + `domains/subscriptions/progress.rs` | split-delete | In-flight because it already speaks runtime tasks, but still overloaded. |
| `core/src/subscription_sync.rs` | transitional | `domains/subscriptions/orchestrator.rs` + `domains/subscriptions/query_engine.rs` + `domains/subscriptions/progress.rs` | split-delete | Keep behavior, delete monolith. |
| `core/src/tag_controller.rs` | legacy-merge | `domains/tags/controller.rs` + `domains/tags/normalize.rs` | merge-delete | Real behavior, wrong top-level ownership. |
| `core/src/tags.rs` | canonical | `domains/tags/normalize.rs` | move | Shared parsing/formatting logic is real. |
| `core/src/types.rs` | legacy-merge | split across `domains/*`, `runtime/*`, `infra/*` | split-delete | Giant bag-of-types file; should not survive intact. |
| `core/src/view_prefs_controller.rs` | legacy-delete | `domains/settings/view_prefs.rs` | delete after call-site move | Thin wrapper only. |
| `core/src/lib.rs` | transitional | `lib.rs` | rewrite | Keep file, but shrink exports to top-level topology only. |

### Dispatch modules

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/dispatch/common.rs` | canonical | `dispatch/common.rs` | keep/refine | Fine as transport helper module. |
| `core/src/dispatch/duplicates.rs` | canonical | `dispatch/duplicates.rs` | keep/slim | Should remain routing-only. |
| `core/src/dispatch/files.rs` | canonical | `dispatch/files.rs` | keep/slim | Domain router after file-domain split. |
| `core/src/dispatch/files_lifecycle.rs` | canonical | `dispatch/files_lifecycle.rs` | keep/slim | Should stop constructing legacy mutation hints. |
| `core/src/dispatch/files_media.rs` | canonical | `dispatch/files_media.rs` | keep/slim | Transport-only. |
| `core/src/dispatch/files_metadata.rs` | canonical | `dispatch/files_metadata.rs` | keep/slim | Transport-only. |
| `core/src/dispatch/files_review.rs` | legacy-delete | `dispatch/files_lifecycle.rs` | delete | One-off review island. This should be folded into lifecycle dispatch. |
| `core/src/dispatch/folders.rs` | canonical | `dispatch/folders.rs` | keep/slim | Routing only after domain split. |
| `core/src/dispatch/grid.rs` | canonical | `dispatch/grid.rs` | keep/slim | Routing only after grid/query split. |
| `core/src/dispatch/mod.rs` | canonical | `dispatch/mod.rs` | keep/refine | Should remain only top-level router. |
| `core/src/dispatch/ptr.rs` | canonical | `dispatch/ptr.rs` | keep/slim | Routing only. |
| `core/src/dispatch/selection.rs` | canonical | `dispatch/selection.rs` | keep/slim | Routing only. |
| `core/src/dispatch/smart_folders.rs` | canonical | `dispatch/smart_folders.rs` | keep/slim | Routing only. |
| `core/src/dispatch/subscriptions.rs` | canonical | `dispatch/subscriptions.rs` | keep/slim | Routing only. |
| `core/src/dispatch/system.rs` | canonical | `dispatch/system.rs` | keep/slim | Routing only. |
| `core/src/dispatch/tags.rs` | canonical | `dispatch/tags.rs` | keep/slim | Routing only. |

### Media processing files

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/files/mod.rs` | legacy-merge | `media_processing/mod.rs` + adapters | split-delete | Too large; should not survive intact. |
| `core/src/files/archive.rs` | canonical | `media_processing/adapters/archive.rs` | move | |
| `core/src/files/blurhash.rs` | canonical | `media_processing/adapters/blurhash.rs` | move | |
| `core/src/files/colors.rs` | canonical | `media_processing/adapters/colors.rs` | move | |
| `core/src/files/ffmpeg.rs` | canonical | `media_processing/adapters/ffmpeg.rs` | move | |
| `core/src/files/ffmpeg_path.rs` | canonical | `media_processing/adapters/ffmpeg_path.rs` | move | |
| `core/src/files/gallery_dl_path.rs` | canonical | `media_processing/adapters/gallery_dl_path.rs` | move | |
| `core/src/files/image_metadata.rs` | canonical | `media_processing/inspect.rs` | move/split | Likely split detect vs inspect responsibilities. |
| `core/src/files/office.rs` | canonical | `media_processing/adapters/office.rs` | move | |
| `core/src/files/pdf.rs` | canonical | `media_processing/adapters/pdf.rs` | move | |
| `core/src/files/specialty.rs` | legacy-merge | `media_processing/adapters/specialty.rs` | split-delete | Large catch-all file; split by actual format families if kept. |
| `core/src/files/svg.rs` | canonical | `media_processing/adapters/svg.rs` | move | |

### SQLite core persistence files

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/sqlite/mod.rs` | transitional | `persistence/connection.rs` + `persistence/mod.rs` | split-delete | Current central DB facade is too broad. |
| `core/src/sqlite/schema.rs` | transitional | `persistence/schema/*` | split-delete | Very large migration pack; keep semantics, delete monolith. |
| `core/src/sqlite/files.rs` | legacy-merge | `domains/files/db.rs` + `persistence/publish/*` | split-delete | Mixes file/entity persistence and projection helpers. |
| `core/src/sqlite/tags.rs` | legacy-merge | `domains/tags/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/folders.rs` | legacy-merge | `domains/folders/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/smart_folders.rs` | legacy-merge | `domains/smart_folders/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/subscriptions.rs` | legacy-merge | `domains/subscriptions/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/flows.rs` | legacy-merge | `domains/flows/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/duplicates.rs` | legacy-merge | `domains/duplicates/db.rs` | split-delete | Domain-owned persistence. |
| `core/src/sqlite/import.rs` | legacy-merge | `domains/files/db.rs` | split-delete | Import-facing persistence should live with files domain. |
| `core/src/sqlite/view_prefs.rs` | legacy-merge | `domains/settings/db.rs` | split-delete | Settings/view prefs belong to settings domain. |
| `core/src/sqlite/collections.rs` | legacy-merge | `domains/files/db.rs` or `domains/collections/db.rs` | split-delete | Depends on final entity/collection ownership; should not stay generic sqlite root. |
| `core/src/sqlite/bitmaps.rs` | canonical | `persistence/publish/bitmaps.rs` | move | Derived read-model infrastructure. |
| `core/src/sqlite/compilers.rs` | canonical | `persistence/publish/compilers.rs` | move | Derived artifact compiler layer. |
| `core/src/sqlite/projections.rs` | canonical | `persistence/publish/projections.rs` | move | Derived projection read model. |
| `core/src/sqlite/sidebar.rs` | canonical | `persistence/publish/sidebar.rs` | move | Sidebar publish/read model infra. |
| `core/src/sqlite/hash_index.rs` | canonical | `persistence/publish/hash_index.rs` or `domains/files/db.rs` | move | Needs final ownership choice; not legacy by itself. |

### PTR persistence files

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/sqlite_ptr/mod.rs` | transitional | `persistence/ptr/mod.rs` + `persistence/ptr/connection.rs` | split-delete | Keep semantics, delete monolith file. |
| `core/src/sqlite_ptr/bootstrap.rs` | legacy-merge | `persistence/ptr/bootstrap.rs` + `domains/ptr/bootstrap.rs` | split-delete | Persistence and orchestration are mixed. |
| `core/src/sqlite_ptr/cache.rs` | canonical | `persistence/ptr/cache.rs` | move | |
| `core/src/sqlite_ptr/overlay.rs` | canonical | `persistence/ptr/overlay.rs` | move | |
| `core/src/sqlite_ptr/sync.rs` | legacy-merge | `persistence/ptr/sync.rs` + `domains/ptr/sync.rs` | split-delete | DB write path and orchestration mixed. |
| `core/src/sqlite_ptr/tags.rs` | legacy-merge | `persistence/ptr/tags.rs` + `domains/ptr/tags.rs` | split-delete | Split persistence from policy. |

### Runtime contract and runtime state files

| Path | Status | Target owner | Action | Notes |
| --- | --- | --- | --- | --- |
| `core/src/runtime_contract/mod.rs` | transitional | `runtime/mod.rs` | merge-delete | Temporary shell while runtime folder does not exist. |
| `core/src/runtime_contract/mutation.rs` | transitional | `runtime/receipts.rs` | move/merge | Canonical direction. |
| `core/src/runtime_contract/snapshot.rs` | transitional | `runtime/snapshot.rs` | move/merge | Canonical direction. |
| `core/src/runtime_contract/task.rs` | transitional | `runtime/task_registry.rs` or `runtime/contracts.rs` | move/merge | Canonical direction. |
| `core/src/runtime_state.rs` | transitional | `runtime/task_registry.rs` + `runtime/sequence.rs` + `runtime/snapshot.rs` | split-delete | Canonical direction, wrong final shape. |

## Highest-confidence delete-now candidates

These files are either pure wrapper shells or obviously superseded transport islands.

| Path | Approx LOC | Why deleteable | Dependency |
| --- | ---: | --- | --- |
| `core/src/sidebar_controller.rs` | 49 | Pure wrapper around DB/sidebar DTO mapping. | Move callers to domain/publish layer. |
| `core/src/import_controller.rs` | 96 | Thin wrapper over `ImportPipeline` + duplicate merge call. | Move dispatch/users to files domain service. |
| `core/src/view_prefs_controller.rs` | 69 | Thin wrapper around DB prefs DTO assembly. | Move callers to settings domain service. |
| `core/src/lifecycle_controller.rs` | ~40 | Thin wrapper over DB/blob lifecycle calls. | Move callers to files lifecycle service. |
| `core/src/dispatch/files_review.rs` | 133 | Legacy one-off dispatch island. | Merge into lifecycle dispatch. |

Immediate delete-now budget after call-site moves: `~350-450 LOC`

## Highest-confidence merge-then-delete candidates

| Path | Approx LOC | Why it should disappear | Replacement shape |
| --- | ---: | --- | --- |
| `core/src/events.rs` legacy sections | 464 | Old invalidation model coexists with new runtime contract. | Runtime event bus only. |
| `core/src/metadata_controller.rs` | 246 | Domain logic stranded at root. | `domains/files/metadata.rs` |
| `core/src/selection_helpers.rs` | 439 | Mixed concerns; helpers are acting as shadow services. | split into selection/grid domain helpers |
| `core/src/types.rs` | 568 | Giant shared bag of unrelated DTOs and domain data. | split by owner |
| `core/src/folder_controller.rs` | 182 | Root controller for folder domain. | fold into `domains/folders/*` |
| `core/src/tag_controller.rs` | 165 | Root controller for tag domain. | fold into `domains/tags/*` |
| `core/src/smart_folder_controller.rs` | 195 | Root controller for smart folder domain. | fold into `domains/smart_folders/*` |
| `core/src/selection_controller.rs` | 360 | Root controller for selection domain. | fold into `domains/selection/*` |
| `core/src/flow_controller.rs` | 413 | Flow orchestration at root. | `domains/flows/*` |
| `core/src/files/mod.rs` | 1054 | Catch-all media-processing bundle. | split into adapters + service modules |
| `core/src/gallery_dl_runner.rs` | 2445 | Large monolith; no clean long-term ownership as a single file. | split into site/process/parser/auth modules |
| `core/src/subscription_controller.rs` | 1107 | Overloaded lifecycle + progress + orchestration. | split into controller/progress/orchestrator |
| `core/src/subscription_sync.rs` | 1576 | Monolithic query execution engine. | split into orchestration/query/progress |
| `core/src/grid_controller.rs` | 1173 | Query service, metadata service, and scope logic mixed. | split grid domain |
| `core/src/sqlite/schema.rs` | 1728 | Monolithic migration pack. | schema pack split |

## Structural guardrails to add after first deletion pass

1. Fail CI if new root-level Rust modules are added outside the approved top-level topology.
2. Fail CI if deleted legacy modules are re-imported.
3. Require every backend restructuring PR to report:
   - modules moved
   - modules deleted
   - legacy markers removed
4. Add a `LEGACY:` marker rule for any temporary compatibility path.
5. Set a soft ceiling on monoliths (`>1200 LOC`) and require explicit justification for growth.

## Deletion campaign sequence

1. Delete thin wrappers and review-path leftovers.
2. Finish runtime contract cutover and delete legacy invalidation/event sections.
3. Fold root controllers into domain folders and delete the old root modules.
4. Split giant monoliths (`gallery_dl_runner`, `subscription_*`, `grid_controller`, `schema`, `files/mod.rs`).
5. Shrink `lib.rs` exports and freeze the root with CI.

## Relationship to existing PBIs

- `PBI-233` through `PBI-314` define the structural and subsystem architecture path.
- The deletion program that falls out of this register should be tracked separately so cleanup does not get deferred behind folder moves forever.
- New PBIs should cover:
  1. the register/classification pass itself
  2. low-risk immediate deletions
  3. legacy event-system purge
  4. controller/root alias purge
  5. root freeze and CI guardrails
