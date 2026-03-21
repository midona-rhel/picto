# PBI-536: mainViewModel dependency array decomposition

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The re-render concern is based on the large dependency array of the `mainViewModel` useMemo in `App.tsx`. Actual re-render impact depends on how frequently these dependencies change and whether child components are memoized (see PBI-517). Profiling with React DevTools should validate whether this causes measurable jank. Related to but distinct from PBI-517 (React.memo audit).

## Priority
P2

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `App.tsx` creates a `mainViewModel` object via `useMemo` with 30+ dependencies. Any change to sidebar state, navigation scope, filter state, or view preferences recreates this object, which flows as a prop or context value to the entire component tree. Without React.memo on child components (see PBI-517), this triggers cascading re-renders on every scope transition.

## Problem
The `mainViewModel` useMemo in `App.tsx` has an excessively large dependency array. Because it combines sidebar data, navigation state, filter state, and view preferences into a single object, a change in *any* of these domains recreates the entire object — even if only one field changed.

This means:
- Clicking a sidebar folder recreates mainViewModel (navigation changed)
- Typing in the search box recreates mainViewModel (filter changed)
- Toggling the inspector recreates mainViewModel (view pref changed)
- Each recreation flows a new object reference to all consumers, triggering re-renders

## Scope
- `src/app/App.tsx` — mainViewModel construction
- Child components that consume mainViewModel

## Implementation
1. **Decompose into domain-specific contexts**: Replace the monolithic `mainViewModel` with separate context providers:
   - `NavigationContext` — current scope, view, folder/tag filters
   - `LayoutContext` — sidebar visible, inspector width, view mode
   - `GridContext` — grid-specific state (page size, sort, zoom)
2. **Each context has its own useMemo**: Smaller dependency arrays mean fewer recreations. A sidebar toggle only recreates `LayoutContext`, not `NavigationContext`.
3. **Consumer components subscribe to specific contexts**: Components that only need layout info don't re-render on navigation changes.
4. **Alternative — keep mainViewModel but split the useMemo**: If context decomposition is too invasive, split the single `useMemo` into 3-4 smaller `useMemo` calls and pass them as separate props. Less elegant but lower effort.

## Acceptance Criteria
1. A sidebar toggle does not cause the grid component to re-render.
2. A navigation change does not cause the toolbar to re-render (unless it displays navigation state).
3. React DevTools Profiler confirms reduced re-render count during common interactions.
4. No visual or behavioral regression.

## Test Cases
1. Profile: Toggle sidebar → only sidebar-dependent components re-render, not grid.
2. Profile: Navigate to folder → grid and breadcrumb re-render, but inspector panel does not.
3. Profile: Type in search box → only filter-dependent components re-render.
4. Profile: Resize inspector → only layout-dependent components re-render.

## Risk
Medium. Context decomposition touches the root component and all its consumers. If mainViewModel is used in many places, the refactor has a wide blast radius. The "split useMemo" alternative is lower risk and may be sufficient if combined with PBI-517 (React.memo on child components).
