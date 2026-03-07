# PBI-405: Frontend legacy deletion campaign and compatibility shim purge

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. The frontend likely contains a substantial amount of legacy code, half-migrations, alias paths, and oversized compatibility surfaces.
2. Past cleanup work has focused more on moving or patching code than deleting obsolete paths.
3. Without an explicit deletion campaign, the topology migration will leave both old and new structures in place.

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
