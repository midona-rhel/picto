# PBI-404: Frontend legacy register and classification pass

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. There is currently no complete ledger of which frontend files are canonical versus transitional or deletable.
2. The codebase still contains mixed ownership between `src/components/*`, `src/features/*`, root entry files, shared helpers, and runtime/bootstrap code.
3. Cleanup work cannot be prioritized properly without a file-by-file classification pass.

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
