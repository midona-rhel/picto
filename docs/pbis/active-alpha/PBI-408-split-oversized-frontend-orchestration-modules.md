# PBI-408: Split oversized frontend orchestration modules

## Priority
P2

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `src/components/sidebar/FolderTree.tsx` is 800+ lines and mixes rendering, DnD, context menus, sorting, CRUD, and auto-tagging.
2. `src/hooks/useInspectorData.ts` is 700+ lines and mixes fetch orchestration, optimistic edits, undo, autosave, and selection summary logic.
3. `src/desktop/api.ts` is 700+ lines and mixes transport, dialogs, window helpers, library host access, and local store behavior.
4. `src/components/FlowsWorking.tsx` remains a large orchestration surface even after subscription runtime work.

## Problem
Several modules are now too large to review safely and mix presentation with orchestration. This is not just a style issue; it slows down changes, encourages coarse tests, and makes regressions harder to localize.

## Scope
- `src/components/sidebar/FolderTree.tsx`
- `src/hooks/useInspectorData.ts`
- `src/desktop/api.ts`
- `src/components/FlowsWorking.tsx`
- Additional oversized modules found during execution

## Implementation
1. Split each target by responsibility, not arbitrary line count:
   - presentation
   - orchestration/controller wiring
   - mutation helpers
   - transport adapters
2. Keep public behavior unchanged while carving out smaller units.
3. Add targeted tests for newly extracted units where practical.
4. Prefer extracting stable helpers/stores/controllers first, then slim the UI shell.

## Acceptance Criteria
1. Each target module has a smaller, clearer ownership surface.
2. Presentation and orchestration are no longer mixed in the same file where avoidable.
3. Extracted units are easier to test and reuse.
4. No visible behavior regressions from the split.

## Test Cases
1. Folder CRUD/DnD still works after `FolderTree` decomposition.
2. Inspector metadata edits still work after `useInspectorData` split.
3. Desktop transport wrappers still function after `src/desktop/api.ts` split.
4. Subscription/flow operations still work after `FlowsWorking` decomposition.

## Risk
Medium. Refactor-only work across large modules can create subtle behavior regressions if not staged carefully.
