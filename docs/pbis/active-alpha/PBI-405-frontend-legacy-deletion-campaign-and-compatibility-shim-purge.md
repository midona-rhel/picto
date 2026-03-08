# PBI-405: Frontend legacy deletion campaign and compatibility shim purge

## Priority
P1

## Status (2026-03-08)
Status: **Implemented**

### Results

1. **Tier 1**: 5 shared hooks moved to `src/shared/hooks/`. `AppErrorBoundary.tsx` moved to `src/shared/components/`.
2. **Tier 2**: 3 shared controllers moved to `src/shared/controllers/` (`fileController`, `undoRedoController`, `perfController`).
3. **Tier 3**: Feature-owned component surfaces moved from `src/components/` into `src/features/*/components/`.
4. **Tier 4**: `src/components/image-grid/` (81 files) moved to `src/features/grid/`. `src/components/` deleted.
5. **Tier 5**: `src/controllers/` (8 files, 30+ consumer imports) moved to `src/shared/controllers/`. `src/controllers/` deleted.
6. **Tier 6**: `src/domain/actions/fileLifecycleActions.ts` (1 file, 3 consumers) moved to `src/shared/controllers/`. `src/domain/` deleted.
7. **Tier 7**: `src/hooks/` (6 files) distributed: inspector hooks → `src/features/inspector/hooks/`, `useGridFeatureState` → `src/features/grid/hooks/`, `useScopedGridPreferences` → `src/shared/hooks/`, dead `useTagEditor` deleted. `src/hooks/` deleted.

All legacy top-level directories (`src/components/`, `src/controllers/`, `src/domain/`, `src/hooks/`) eliminated. `npx tsc --noEmit` passes clean.

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
