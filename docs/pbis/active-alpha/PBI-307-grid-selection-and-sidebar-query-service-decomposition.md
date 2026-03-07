# PBI-307: Grid, selection, and sidebar query service decomposition

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `core/src/grid_controller.rs` is 1100+ lines and acts as a multi-scope query service, filter engine, and read-path coordinator.
2. `selection_helpers.rs` contains substantial query and summary logic outside a dedicated read service.
3. Sidebar counts and sidebar projections live partly in controller code and partly in SQLite helper modules.
4. Read-side dependencies between grid, selection, sidebar, tags, and status scopes are hard to trace.

## Problem
The backend read side is split by historical controller boundaries instead of by coherent query services. Grid, selection, and sidebar behavior all depend on overlapping read models, but ownership is not centralized. This makes read-path correctness, performance work, and runtime invalidation harder to reason about.

## Scope
- `core/src/grid_controller.rs`
- `core/src/selection_controller.rs`
- `core/src/selection_helpers.rs`
- `core/src/sidebar_controller.rs`
- relevant `sqlite/sidebar.rs` and projection helpers

## Implementation
1. Define explicit query services for:
   - grid/page queries
   - selection summaries
   - sidebar snapshots/counts
2. Pull shared read logic out of controller glue.
3. Make scope semantics explicit and reusable between grid/selection/sidebar.
4. Prepare the backend read side for the runtime resource model introduced by `PBI-234`.

## Acceptance Criteria
1. Grid, selection, and sidebar read logic have clearer service boundaries.
2. Controllers become thin entry points rather than mixed query engines.
3. Shared scope/read semantics are defined once.
4. Future runtime invalidation can target read services cleanly.

## Test Cases
1. Grid paging behavior remains unchanged.
2. Selection summaries still match current results.
3. Sidebar counts and structure remain correct.

## Risk
Medium-high. Large query surface with performance sensitivity.
