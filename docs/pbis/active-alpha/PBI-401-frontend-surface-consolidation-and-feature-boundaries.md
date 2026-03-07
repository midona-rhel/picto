# PBI-401: Frontend surface consolidation and feature boundary enforcement

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `src/App.tsx` imports from both feature-first surfaces and legacy direct component/service paths.
2. The codebase still has parallel ownership trees such as `src/components/sidebar/` and `src/features/sidebar/`.
3. High-value screens like flows, tags, collections, duplicates, and parts of the sidebar still live primarily in legacy `src/components/` paths.

## Problem
The frontend migration to feature-first structure is incomplete. New code can still land in the legacy tree because the boundary is not explicit or enforced, and imports do not reliably signal domain ownership.

## Scope
- `src/components/`
- `src/features/`
- `src/App.tsx`
- `src/services/`
- Barrel exports and import paths that still cross legacy/feature boundaries

## Implementation
1. Define the intended ownership boundary per domain:
   - grid
   - sidebar
   - subscriptions
   - tags
   - folders
   - settings
   - duplicates
   - collections
2. Move remaining domain-owned screens/components behind `src/features/*` entry points.
3. Reduce direct `src/components/*` imports from app shell and top-level screens.
4. Leave `src/components/*` only for shared presentational primitives that are genuinely cross-domain.
5. Add or tighten barrel exports so import paths communicate ownership clearly.

## Acceptance Criteria
1. Domain-owned screens/components are imported through feature surfaces, not legacy paths.
2. `src/App.tsx` no longer mixes feature surfaces with ad hoc legacy domain imports.
3. The remaining `src/components/*` tree is clearly shared/presentational, not a second domain tree.
4. Future contributors can infer domain ownership from file location and import path.

## Test Cases
1. Inspect imports in `src/App.tsx` and domain entrypoints — feature-owned modules come from `src/features/*`.
2. Move/update one domain module without touching unrelated legacy import chains.
3. Run `npx tsc -p tsconfig.json --noEmit` after the move set.

## Risk
Medium. Mostly file moves and import cleanup, but broad enough to create churn if done without a clear domain-by-domain sequence.
