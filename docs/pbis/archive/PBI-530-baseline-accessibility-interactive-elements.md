# PBI-530: Baseline accessibility for interactive elements

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The accessibility gap was identified by searching for `aria-` attributes (11 total across the entire frontend). For a desktop image management app, full WCAG 2.1 compliance may not be a priority. This PBI proposes a minimal baseline — not full compliance — focused on keyboard navigability and screen reader support for primary workflows.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: Only 11 `aria-*` attributes found across the entire `src/` directory. The canvas-based grid is opaque to screen readers. No ARIA roles on toolbar buttons, sidebar tree items, or context menus. Focus management exists for keyboard shortcuts but not for standard tab navigation.

## Problem
The frontend has near-zero accessibility support:

1. **Canvas grid**: Completely invisible to screen readers — no alt text, no ARIA live region for selection changes
2. **Interactive elements**: Toolbar buttons, sidebar items, and panel controls lack `role` and `aria-label` attributes
3. **Focus management**: Tab order is not explicitly managed — focus can get lost in complex layouts
4. **Context menus**: Custom context menus lack ARIA menu roles

For the current user base (visual media management), this may be acceptable. But it would block any adoption context requiring basic WCAG compliance.

## Scope
- `src/app/App.tsx` — landmark roles (main, nav, aside)
- `src/features/grid/ImageGrid.tsx` — canvas accessibility layer
- `src/app-shell/Sidebar.tsx` — tree navigation roles
- `src/shared/styles/iconButton.module.css` — button accessibility
- All toolbar and icon button components

## Implementation
1. **Add ARIA landmark roles**: `role="main"` on content area, `role="navigation"` on sidebar, `role="complementary"` on inspector panel. These help screen readers understand page structure.
2. **Add `aria-label` to all icon buttons**: Every `.icBtn` element that lacks visible text needs an `aria-label`. Audit all icon buttons and add labels matching the existing `KbdTooltip` label text.
3. **Sidebar tree roles**: Add `role="tree"`, `role="treeitem"`, `aria-expanded` to sidebar folder/smart folder nodes.
4. **Canvas accessibility overlay**: Add a hidden `<div role="grid" aria-label="Image grid">` that shadows the canvas grid. Each visible tile gets a hidden `<div role="gridcell" aria-label="{filename}">`. This enables screen readers to announce grid contents without changing the visual rendering.
5. **Context menu roles**: Add `role="menu"` and `role="menuitem"` to custom context menu components.

## Acceptance Criteria
1. Landmarks defined: screen readers can navigate between sidebar, content, and inspector.
2. All icon buttons have `aria-label` matching their tooltip text.
3. Sidebar tree items have `role="treeitem"` and `aria-expanded` state.
4. Screen reader can announce the currently selected grid item.
5. No visual changes — all accessibility additions are semantic only.

## Test Cases
1. VoiceOver (macOS) reads sidebar structure as a tree with expandable nodes.
2. Tab key navigates between sidebar, grid, and inspector without getting stuck.
3. Screen reader announces grid selection changes via ARIA live region.
4. All toolbar buttons announced by name (not "button" or silence).
5. Context menu items announced by name when navigated with arrow keys.

## Risk
Low. ARIA attributes are additive and non-visual. The canvas accessibility overlay is the highest-effort item and could be deferred if the simpler items provide sufficient baseline coverage.
