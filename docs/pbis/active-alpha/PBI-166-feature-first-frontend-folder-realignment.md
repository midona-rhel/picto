# PBI-166: Complete frontend feature-first folder realignment

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

## Problem
Frontend structure is partially domainized (`features/inspector`) but most code remains in large cross-domain `components/*`, making ownership and entrypoints unclear for new contributors.

## Scope
- `/Users/midona/Code/imaginator/src/components/*`
- `/Users/midona/Code/imaginator/src/features/*`
- `/Users/midona/Code/imaginator/src/controllers/*`
- `/Users/midona/Code/imaginator/src/stores/*`

## Implementation
1. Move to domain folders under `src/features/`:
   - `grid`, `sidebar`, `folders`, `tags`, `duplicates`, `subscriptions`, `settings`, `viewer`.
2. Per feature enforce structure:
   - `components/`
   - `store/`
   - `controller/` (or facade imports)
   - `types/`
   - `hooks/`
3. Keep `src/components/ui` as shared presentation primitives only.
4. Add path alias conventions + import lint rules to discourage cross-domain deep imports.

## Acceptance Criteria
1. Engineers can find feature code by domain immediately.
2. Shared UI primitives are separated from domain behavior components.
3. Deep cross-feature imports are reduced and linted.

## Test Cases
1. Build/test pass after staged moves.
2. Main flows (grid/sidebar/settings/detail) run with unchanged behavior.

## Risk
Medium-high. High-touch move/refactor; do in phased batches.

