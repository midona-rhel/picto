# PBI-087: Centralize frontend domain mutations and refresh policy

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. UI components still execute domain mutations directly and manually fan out sidebar refreshes:
   - `/Users/midona/Code/imaginator/src/components/sidebar/FolderTree.tsx` (many `FolderController.*` + `SidebarController.requestRefresh()` calls)
   - `/Users/midona/Code/imaginator/src/components/image-grid/ImageGrid.tsx` (many mutation call-sites with post-mutation refresh chaining)
   - `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx:130`
   - `/Users/midona/Code/imaginator/src/hooks/useInspectorData.ts:344,360`
2. `SidebarController.requestRefresh()` is still called across many files:
   - `/Users/midona/Code/imaginator/src/components`, `/Users/midona/Code/imaginator/src/hooks`, `/Users/midona/Code/imaginator/src/stores`

## Problem
Mutation semantics (perform operation + invalidate cache/sidebar + optional optimistic state) are duplicated in many UI components. This causes inconsistent behavior, makes bugs hard to fix globally, and blocks clear ownership boundaries.

## Scope
- `/Users/midona/Code/imaginator/src/components/sidebar/FolderTree.tsx`
- `/Users/midona/Code/imaginator/src/components/image-grid/ImageGrid.tsx`
- `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx`
- `/Users/midona/Code/imaginator/src/hooks/useInspectorData.ts`
- new shared mutation layer under `/Users/midona/Code/imaginator/src/domain/actions/`

## Implementation
1. Add a shared domain action layer (`folderActions`, `smartFolderActions`, `fileActions`) that owns:
   - backend command execution
   - optimistic local store updates (where safe)
   - cache/sidebar invalidation policy
2. Replace direct `FolderController.*`/`SidebarController.requestRefresh()` usage in components with action calls.
3. Centralize error handling strategy (toast + rollback policy) in action layer.
4. Add lint rule or static check: no component file may call `SidebarController.requestRefresh()` directly.

## Acceptance Criteria
1. Components do not manually orchestrate mutation + refresh fanout.
2. Refresh/invalidation behavior is consistent across sidebar, subfolder grid, and image grid actions.
3. New mutation flows are added in one place (domain action layer), not per component.

## Test Cases
1. Folder create/rename/delete/reorder from both sidebar and subfolder grid update UI consistently.
2. Folder add/remove batch from image grid updates counts and views without manual refresh chains.
3. Error in mutation path shows consistent UI error behavior and no stale partial state.

## Risk
Medium. Touches many high-frequency interaction paths but gives major maintainability gains.

