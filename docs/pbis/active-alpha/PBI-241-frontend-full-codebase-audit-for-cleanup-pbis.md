# PBI-241: Frontend full codebase audit for cleanup PBIs

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. PBI-166 (frontend feature-first folder realignment) defines the target structure but doesn't catalogue the specific issues in each module.
2. Existing active-alpha PBIs (PBI-087, PBI-088, PBI-092, PBI-095, PBI-098, PBI-099, PBI-102) address known architectural problems but were written from a high-level perspective, not a line-by-line audit.
3. The frontend has accumulated the same patterns of drift as the backend: applied fixes without cleanup, redundant components, inconsistent state management, stale workarounds.
4. Components like ImageGrid/CanvasGrid have known TypeScript compile contract mismatches (from the implementation audit).

## Problem
The frontend codebase has not been systematically audited for technical debt. Existing PBIs cover known architectural issues but not the full scope of accumulated problems:
- Dead or redundant components
- Inconsistent state management patterns (some in Zustand stores, some in local state, some prop-drilled)
- Stale hooks and utilities that are no longer used
- Components that have drifted from their intended responsibility
- Inconsistent styling approaches (inline styles vs CSS modules vs Mantine)
- Type safety gaps (any casts, missing types, loose interfaces)

## Scope
- Every file in `src/` — components, stores, hooks, utils, types, services, controllers
- The Electron main process (`desktop/`)
- Output: a set of new PBIs (or additions to existing PBIs) for each issue found

## Implementation
1. Go through every directory in `src/` systematically:
   - `src/components/` — each component directory
   - `src/features/` — each feature directory
   - `src/stores/` — every store
   - `src/hooks/` — every hook
   - `src/controllers/` — every controller
   - `src/services/` — every service
   - `src/utils/` and `src/lib/` — every utility
   - `src/types/` — every type definition
   - `src/contexts/` — every context
   - `desktop/` — Electron main process
2. For each file/component, document:
   - **What it's supposed to do**
   - **What it actually does**
   - **Gaps**: dead code, redundant components, stale hooks, unused imports, type safety issues
   - **Drift**: components that have grown beyond their original scope, inconsistent patterns
3. For each issue found, either:
   - Create a new PBI
   - Append to an existing PBI (especially PBI-166, PBI-087, PBI-088, etc.)
4. Produce a summary audit report.

## Directories to audit (checklist)

### Components
- [ ] `src/components/image-grid/`
- [ ] `src/components/sidebar/`
- [ ] `src/components/detail-view/`
- [ ] `src/components/settings/`
- [ ] `src/components/subscriptions/`
- [ ] `src/components/tags/`
- [ ] `src/components/duplicates/`
- [ ] `src/components/collections/`
- [ ] `src/components/` (any remaining)

### Features
- [ ] `src/features/` (all subdirectories)

### State & Logic
- [ ] `src/stores/`
- [ ] `src/hooks/`
- [ ] `src/controllers/`
- [ ] `src/services/`
- [ ] `src/contexts/`

### Shared
- [ ] `src/utils/`
- [ ] `src/lib/`
- [ ] `src/types/`
- [ ] `src/styles/`

### Desktop
- [ ] `desktop/` (Electron main process)

### Root
- [ ] `src/App.tsx`
- [ ] `src/main.tsx`
- [ ] `src/detail.tsx`
- [ ] `src/settings.tsx`
- [ ] `src/subscriptions.tsx`
- [ ] `src/library-manager.tsx`

## Acceptance Criteria
1. Every directory and significant file in `src/` has been reviewed.
2. All identified issues are tracked as PBIs (new or appended to existing).
3. A summary audit report exists listing areas reviewed and findings.
4. Existing architectural PBIs (PBI-087, PBI-088, PBI-166, etc.) are updated with specific findings where applicable.
5. No known frontend technical debt is left uncatalogued.

## Test Cases
1. Audit report is complete — every directory has an entry.
2. All new PBIs have clear scope and acceptance criteria.

## Risk
Low (it's an audit, not a code change). Time investment is medium-high (~1-2 days of focused review). The frontend has more surface area than the backend due to UI components.
