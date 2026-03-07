# PBI-405: Frontend legacy deletion campaign and compatibility shim purge

## Priority
P1

## Audit Status (2026-03-07)
Status: **Partially Implemented — Tiers 1 & 2 Complete**

Completed:
1. Tier 1 deletion wave: 5 shared hooks moved from `src/hooks/` to `src/shared/hooks/`.
2. `AppErrorBoundary.tsx` moved from `src/components/` to `src/shared/components/`.
3. Associated test file moved to `src/shared/hooks/__tests__/`.
4. Tier 2 deletion wave: 3 shared controllers moved from `src/controllers/` to `src/shared/controllers/`.
   - `fileController.ts` (9 consumers updated)
   - `undoRedoController.ts` (15 consumers updated)
   - `perfController.ts` (1 consumer updated)
5. All consumer imports updated, builds clean.

Remaining:
1. Tier 3: feature migration (move components into features/).
2. Tier 4: image-grid split (blocked on PBI-408).

## Problem
The project needs a deletion program, not just a migration program. If old paths remain after replacements land, the codebase will keep two ownership models alive indefinitely.

## Scope
- Frontend legacy register outputs from `PBI-404`
- Compatibility shims, alias modules, and duplicate domain surfaces
- Deletion-budget policy for cleanup PRs

## Implementation
1. Define the first deletion wave from the legacy register:
   - delete now
   - merge then delete
   - blocked
2. Remove compatibility wrappers and alias modules once replacements are active.
3. Require cleanup PRs to delete code, not only move it.
4. Track deletion progress against an explicit LOC budget.

## Acceptance Criteria
1. The first deletion wave removes real legacy paths, not just comments.
2. Cleanup PRs demonstrate net legacy reduction.
3. Old paths are removed when replacements land.
4. Frontend structure moves toward one canonical ownership model.

## Test Cases
1. Delete a migrated legacy module and confirm the canonical replacement path is used everywhere.
2. Remove an obsolete alias path and confirm builds/tests still pass.
3. Track net deletion across the first cleanup wave.

## Risk
Medium. Real deletion is where hidden dependencies surface, but that is exactly why it needs to happen explicitly.
