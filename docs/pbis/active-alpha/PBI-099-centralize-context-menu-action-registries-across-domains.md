# PBI-099: Centralize context-menu action registries across domains

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Context menu builders are still implemented per feature surface:
   - `/Users/midona/Code/imaginator/src/components/sidebar/FolderTree.tsx`
   - `/Users/midona/Code/imaginator/src/components/sidebar/SmartFolderList.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/ImageGrid.tsx`
   - `/Users/midona/Code/imaginator/src/components/TagManager.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/FilterBar.tsx`
2. Action labels/ordering and behavior diverge between equivalent contexts.

## Problem
Menu actions are duplicated and inconsistent. Adding/changing one action requires touching many components and creates behavior drift.

## Scope
- files listed above
- new shared registries under `/Users/midona/Code/imaginator/src/components/ui/context-actions/`

## Implementation
1. Define domain action registries:
   - `folderActions`
   - `smartFolderActions`
   - `imageActions`
   - `tagActions`
2. Build menus by composing shared action definitions + context filters (single/multi/root/background).
3. Keep component-specific placement/anchor logic only.
4. Add consistency tests for action availability matrix by context.

## Acceptance Criteria
1. Context-menu behavior for equivalent entities is consistent across views.
2. Action definitions are centralized and reused.
3. New actions are added once and automatically available where valid.

## Test Cases
1. Folder context menu parity between sidebar and subfolder grid.
2. Image context menu parity between grid and detail contexts where applicable.
3. Tag context menu actions preserve existing behavior after registry migration.

## Risk
Medium. Cross-surface refactor; strong consistency and reuse gains.

