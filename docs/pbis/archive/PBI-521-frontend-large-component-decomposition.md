# PBI-521: Frontend large component decomposition

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The file size thresholds are based on static line counts. Large files are not inherently problematic if they have a single clear responsibility. Some of these components may be intentionally monolithic because splitting them would increase prop-passing complexity without improving readability. Human review should determine which splits are actually worthwhile.

## Priority
P3

## Problem
Several frontend components exceed 600 lines and contain multiple distinct concerns:

| Component | Lines | Concerns |
|-----------|-------|----------|
| `src/features/grid/ImageGrid.tsx` | 863 | Grid orchestration, 22 hook composition, prop forwarding |
| `src/features/tags/TagSelectPanel.tsx` | 809 | Tag picker, namespace grouping, search, hierarchy navigation |
| `src/features/viewer/components/MediaView.tsx` | 719 | Carousel navigation, zoom/pan, keyboard shortcuts, video/image switching |
| `src/features/duplicates/components/DuplicateManager.tsx` | 663 | Duplicate pair display, zoom/pan, resolution workflow, navigator |
| `src/features/viewer/components/DetailWindow.tsx` | 610 | Detail view lifecycle, metadata panel, keyboard shortcuts |
| `src/features/sidebar/SmartFolderList.tsx` | 600 | Smart folder tree, edit modal, predicate UI |

Note: ImageGrid already uses 22 hooks for decomposition — the file is large because it orchestrates many hooks, not because logic is inlined. This may not need further splitting.

## Scope
- Review each component to identify natural split boundaries
- Extract sub-components where the split clearly improves readability
- Do NOT split components where the separation would be artificial

## Implementation

### Likely good splits
1. **SmartFolderList.tsx** → `SmartFolderTree.tsx` + `SmartFolderEditModal.tsx` — the edit modal is a distinct UI that could be a sibling component.
2. **TagSelectPanel.tsx** → `TagNamespaceGroup.tsx` + `TagSearchInput.tsx` — the namespace group rendering and search are reusable sub-units.

### Needs investigation
3. **MediaView.tsx** — Zoom/pan logic could move to a shared hook (see PBI-518). Keyboard shortcuts could move to a dedicated handler. But carousel + zoom are tightly coupled — splitting may not help.
4. **DuplicateManager.tsx** — Similar to MediaView; zoom/pan extraction (PBI-518) would reduce this naturally.

### Probably leave as-is
5. **ImageGrid.tsx** — Already decomposed via hooks. The file is large because it's the orchestrator. Further splitting would just move the hook composition to another file.
6. **DetailWindow.tsx** — Single responsibility (detail window lifecycle). Large but coherent.

## Acceptance Criteria
1. Extracted components render identically to before.
2. No new prop drilling introduced (extracted components receive props from parent, not through intermediaries).
3. `npx tsc --noEmit` passes.
4. `npm run guard:feature-facades` passes.

## Test Cases
1. Smart folder edit modal opens and saves correctly after extraction.
2. Tag picker search and namespace navigation work after extraction.
3. All existing tests pass.

## Risk
Low. Component extraction is non-destructive if done carefully. The risk is creating components that are too granular, increasing the import graph without improving readability.
