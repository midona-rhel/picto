# PBI-085: Extract shared global interaction hooks (keyboard/pointer lifecycle)

## Priority
P1

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Resolved:
1. Created `useGlobalKeydown` hook (`src/hooks/useGlobalKeydown.ts`) — standardizes document keydown listener attach/detach with capture option and enabled gate.
2. Created `useGlobalPointerDrag` hook (`src/hooks/useGlobalPointerDrag.ts`) — standardizes isDragging → mousemove/mouseup lifecycle.
3. Migrated 3 overlay components to use `useGlobalKeydown`: ContextMenu, TagPickerMenu, UrlListEditor.

Remaining:
- Migrate remaining keydown consumers: DetailWindow, QuickLook, DuplicateManager, ImageGridControls, TagSelectPanel, useAppBootstrap.
- Migrate isDragging-gated drag consumers to `useGlobalPointerDrag`: useImageZoom, ZoomableImage, VideoPlayer.
- TagSelectPanel drag uses unconditional listener pattern (ref-gated, not state-gated) — needs restructuring to fit hook.

## Problem
Global interaction patterns are duplicated per component, increasing the risk of inconsistent teardown, stale closures, and subtle interaction bugs.

## Scope
- Shared hook module under `/Users/midona/Code/imaginator/src/hooks/` (new)
- Components currently owning direct `window/document.addEventListener` lifecycles

## Implementation
1. Create shared hooks:
   - `useGlobalKeydown(handler, deps, options)`
   - `useGlobalPointerDrag({ onMove, onEnd, pointerId })`
   - optionally `useGlobalClickOutside(...)`
2. Migrate highest-risk components first (ContextMenu, TagSelectPanel, TagPickerMenu, VirtualGrid drag).
3. Standardize capture/passive options and teardown behavior.
4. Add regression tests for attach/detach behavior under mount/unmount churn.

## Acceptance Criteria
1. Target components no longer implement bespoke global listener plumbing.
2. Listener cleanup is deterministic across remount/HMR transitions.
3. Keyboard and drag interactions remain behaviorally identical.

## Test Cases
1. Mount/unmount overlay components repeatedly; verify no listener leaks.
2. Drag start/cancel/end paths clean up global pointer listeners.
3. Escape key closes overlays consistently via shared hook.

## Risk
Medium. Cross-cutting interaction-layer refactor.

