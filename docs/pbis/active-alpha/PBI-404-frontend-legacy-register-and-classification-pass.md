# PBI-404: Frontend legacy register and classification pass

## Priority
P1

## Audit Status (2026-03-07)
Status: **Implemented**

Implementation:
1. `docs/frontend-legacy-register.md` — comprehensive file-by-file classification.
2. Every major frontend area classified as canonical, transitional, or legacy-merge.
3. Each non-canonical item has a target owner and delete condition.
4. Four-tier deletion priority: move-now, merge-shared, feature-migration, architectural-split.

## Problem
The frontend has too much legacy and no single source of truth for what should be kept, merged, or deleted. That makes deletion reactive instead of systematic.

## Scope
- `docs/frontend-legacy-register.md`
- Entire current frontend tree under `src/`

## Implementation
1. Expand the seed legacy register into a real ledger.
2. Classify frontend files/folders into:
   - `canonical`
   - `transitional`
   - `legacy-delete`
   - `legacy-merge`
3. Record target owner/replacement path for each non-canonical item.
4. Produce a ranked deletion candidate list:
   - delete now
   - merge then delete
   - blocked on topology move

## Acceptance Criteria
1. Every major frontend area has been classified.
2. The legacy register is concrete enough to drive deletion work, not just describe drift.
3. The codebase can answer “what should we delete next?” without rediscovery.

## Test Cases
1. Open the legacy register and find a classification for each major frontend bucket.
2. Each non-canonical item has a target owner and delete condition.

## Risk
Low. Documentation/inventory work only, but it must be concrete to be useful.
