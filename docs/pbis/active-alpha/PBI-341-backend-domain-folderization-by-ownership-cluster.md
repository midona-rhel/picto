# PBI-341: Backend domain folderization by ownership cluster

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. Domain logic is split across flat files like `folder_controller.rs`, `tag_controller.rs`, `ptr_controller.rs`, `subscription_controller.rs`, `grid_controller.rs`, and related helpers.
2. Domain-adjacent files are not physically grouped, so ownership is unclear.
3. Existing `PBI-233` identifies the problem but does not break the move into execution clusters.

## Problem
The backend needs a staged physical move of root-level domain files into domain folders. Without that, the architecture stays theoretical and the root remains a flat row of files.

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Scope
- `core/src/domains/*`
- current root-level domain/controller/helper files

## Implementation
1. Move domains in explicit clusters:
   - cluster A: `tags`, `folders`, `smart_folders`
   - cluster B: `selection`, `grid`, `duplicates`
   - cluster C: `files` / import / lifecycle / metadata
   - cluster D: `subscriptions`, `flows`
   - cluster E: `ptr`
   - cluster F: `settings`
2. Each domain folder gets `mod.rs` and internal ownership boundaries.
3. Delete old root-level files after each cluster is complete.

## Acceptance Criteria
1. No domain/controller files remain directly in `core/src/` after full completion.
2. Each domain cluster is navigable in one folder.
3. Partial moves are done cluster-by-cluster, not as one giant unsafe diff.

## Test Cases
1. Build/tests pass after each cluster.
2. Smoke test relevant domain after each cluster move.

## Risk
High. Many file moves, but manageable if staged strictly by cluster.
