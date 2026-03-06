# PBI-081: Centralize folder context-menu action registry across sidebar and subfolder grid

## Priority
P1

## Audit Status (2026-03-03)
Status: **Implemented**

Evidence:
1. Shared registry added:
   - `/Users/midona/Code/imaginator/src/components/sidebar/contextMenuRegistry.tsx`
2. FolderTree now routes through shared builders:
   - `/Users/midona/Code/imaginator/src/components/sidebar/FolderTree.tsx`
3. SubfolderGrid now routes through shared builders:
   - `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx`
4. SmartFolderList now routes through shared builder:
   - `/Users/midona/Code/imaginator/src/components/sidebar/SmartFolderList.tsx`
5. Parity/structure tests added:
   - `/Users/midona/Code/imaginator/src/components/sidebar/__tests__/contextMenuRegistry.test.tsx`

## Problem
Folder/smart-folder context-menu behavior is duplicated in multiple components, causing option mismatch, inconsistent enablement, and repeated bug-fix effort.

## Scope
- `/Users/midona/Code/imaginator/src/components/sidebar/FolderTree.tsx`
- `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx`
- `/Users/midona/Code/imaginator/src/components/sidebar/SmartFolderList.tsx`
- new shared menu/action module under `/src/components/sidebar/` (or `/src/features/folders/`)

## Implementation
1. Create a shared action registry/factory for folder menus:
   - single-folder
   - multi-folder
   - empty-grid/parent-surface
   - smart-folder item
2. Keep UI layer responsible only for context and selection; route actions through shared commands.
3. Encode feature flags/capabilities in one place (hide unsupported actions instead of per-component placeholders).
4. Add parity tests/snapshots for menu structures by scenario.

## Acceptance Criteria
1. Folder actions and labels are consistent across sidebar and subfolder grid.
2. Unsupported options do not appear inconsistently in one surface only.
3. New folder action changes require edits in one shared location.

## Test Cases
1. Compare menu entries for single-folder right-click in sidebar vs subfolder grid.
2. Multi-select folder menus expose the same supported batch actions.
3. Parent/empty-space menu includes expected create actions and no duplicate separators.

## Risk
Medium. Cross-component refactor touching interaction-heavy code paths.
