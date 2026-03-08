# PBI-341: Backend domain folderization by ownership cluster

## Priority
P1

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. Major domains are now folderized: `folders`, `tags`, `smart_folders`, `grid`, `selection`, `duplicates`, `metadata`, and `subscriptions`.
2. The biggest flat-root domain files named in the original audit are already gone or moved.
3. The remaining problem is no longer “folderize anything at all”; it is finishing ownership-correct moves for persistence, runtime, and leftover cross-cutting services.

## Problem
The backend needs a staged physical move of root-level domain files into domain folders. Without that, the architecture stays theoretical and the root remains a flat row of files.

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Scope
- current domain folders and any remaining flat-root domain/controller/helper files

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
