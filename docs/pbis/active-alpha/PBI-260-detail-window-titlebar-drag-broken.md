# PBI-260: Detail window titlebar drag is broken

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. The detached detail window opens correctly, but dragging the custom titlebar/toolbar does not move the window.
2. This is a direct regression in the host-window interaction path, not a renderer navigation issue.
3. The main window already supports drag regions correctly, so the detail window behavior is inconsistent.

## Problem
The detail window cannot be dragged around by its custom titlebar area. This makes the detached viewer feel broken and undermines the custom frameless-window experience.

## Scope
- `/Users/midona/Code/imaginator/src/components/image-grid/DetailWindow.tsx`
- `/Users/midona/Code/imaginator/electron/`
- Any preload or window IPC bridge involved in drag start behavior

## Implementation
1. Audit the current detail window drag path and confirm whether it relies on:
   - CSS app-region drag
   - explicit `startDragging()` host API call
   - an event target exclusion rule that is over-matching
2. Fix the drag initiation path so the detail window can be dragged from the intended toolbar/title area.
3. Preserve exclusions for interactive controls like buttons and inputs.
4. Add a lightweight regression check or manual verification note to ensure frameless detail windows remain draggable after future host refactors.

## Acceptance Criteria
1. The detail window can be dragged from its intended titlebar/toolbar area.
2. Clicking toolbar buttons still works and does not start a drag.
3. Dragging behavior is consistent with the rest of the frameless window model.

## Test Cases
1. Open a detail window and drag from empty toolbar space; the window moves.
2. Click zoom/pin/close controls; they activate normally and do not drag the window.
3. Re-open the detail window after restart; drag still works.

## Risk
Low. Small interaction fix, but it touches the frameless window behavior path.
