# PBI-517: Frontend render optimization — React.memo audit

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The performance concern is based on the absence of `React.memo()` in the codebase, but actual re-render impact depends on component tree structure and state update patterns that were not profiled at runtime. This may be a non-issue in practice — profiling should determine whether action is needed before any code is changed.

## Priority
P2

## Problem
`React.memo()` is not used anywhere in the frontend (zero matches for `React.memo` across all of `src/`). Several large, frequently-rendered components may be re-rendering unnecessarily when parent state changes:

- `CanvasGrid.tsx` (656 LOC) — renders the grid canvas, called on every grid state change
- `MediaView.tsx` (719 LOC) — detail view carousel with zoom/pan/keyboard
- `DetailWindow.tsx` (610 LOC) — floating detail window
- `ImageGrid.tsx` (863 LOC) — main grid orchestrator with 22 hooks

For a desktop image management app, unnecessary re-renders during grid scrolling, detail view transitions, or sidebar interactions could cause perceptible jank.

Similarly, `React.lazy()` is not used — all features load eagerly. For an Electron app this is less critical than web, but lazy-loading settings, subscriptions, and duplicate panels could improve startup time.

## Scope
- Profile the app to identify actual re-render hotspots
- Apply `React.memo()` where profiling shows measurable benefit
- Consider `React.lazy()` for rarely-used panels (settings, subscriptions, duplicates)

## Implementation
1. **Profile first**: Use React DevTools Profiler to record interactions (grid scroll, detail view open/close, sidebar click, tag editing). Identify components that re-render without prop changes.
2. **Apply React.memo()** to leaf components that profiling shows re-render unnecessarily. Likely candidates:
   - `CanvasGrid` — heavy render, should only update on grid data/layout changes
   - Individual media card sub-components (`MediaCardFrame`, `MediaCardImage`, `MediaCardOverlay`)
   - Inspector panel sub-components
3. **Consider useMemo/useCallback audit** for expensive derived values or callbacks passed as props.
4. **Consider React.lazy()** for `SettingsPanel`, `SubscriptionsWindow`, `DuplicateManager` — these are opened rarely and could be code-split.

## Acceptance Criteria
1. React DevTools Profiler shows no unnecessary re-renders during grid scrolling.
2. Grid scroll performance is at least as smooth as before (no regressions).
3. Detail view transitions do not trigger grid component re-renders.
4. Sidebar interactions do not trigger detail view re-renders.

## Test Cases
1. Profile: Grid scroll → only visible tiles and canvas re-render, not the entire component tree.
2. Profile: Open detail view → grid stops re-rendering while detail is active.
3. Profile: Click sidebar folder → grid re-renders for new data, but header/toolbar do not.
4. If React.lazy applied: Settings panel loads on first open, not at app startup.

## Risk
Low. React.memo is additive and non-breaking. The main risk is premature optimization — hence the "profile first" requirement. Over-memoizing can increase memory usage and make debugging harder.
