# PBI-305: Grid, selection, and sidebar query service decomposition

## Priority
P1

## Audit Status (2026-03-08)
Status: **Implemented**

### Delivered
- Extracted grid page query logic (scope resolution, pagination, color filtering) into `core/src/grid/query.rs`
- Extracted metadata batch prefetch + PTR merge into `core/src/grid/metadata.rs`
- Extracted selection mutations (add/remove tags, rating, notes, URLs) into `core/src/selection/mutations.rs`
- Extracted selection summary query into `core/src/selection/summary.rs`
- `grid/controller.rs` and `selection/controller.rs` reduced to thin delegators preserving public API
- Sidebar already clean (55 lines) — no changes needed
- All 302 Rust tests pass, TypeScript clean

## Problem
The backend read side is split by historical controller boundaries instead of by coherent query services. Grid, selection, and sidebar behavior all depend on overlapping read models, but ownership is not centralized. This makes read-path correctness, performance work, and runtime invalidation harder to reason about.

## Scope
- `core/src/grid_controller.rs`
- `core/src/selection_controller.rs`
- `core/src/selection_helpers.rs`
- `core/src/sidebar_controller.rs`
- relevant `sqlite/sidebar.rs` and projection helpers

## Dependencies
Depends on:
1. `PBI-300` for canonical scope semantics.
2. `PBI-303` for the model-fact to derived-resource dependency contract.
3. `PBI-301` for the business-logic conformance suite that locks scope behavior before service extraction.

## Not In Scope
1. Redefining business rules for `select all`, `untagged`, `uncategorized`, or inbox visibility.
2. Replacing runtime event transport.
3. Frontend store/controller invalidation behavior.

## Implementation
1. Define explicit query services for:
   - grid/page queries
   - selection summaries
   - sidebar snapshots/counts
2. Pull shared read logic out of controller glue.
3. Consume the canonical scope resolver introduced by `PBI-300` rather than re-implementing scope semantics locally.
4. Separate:
   - scope resolution
   - entity-id population queries
   - page/cursor materialization
   - selection summary aggregation
   - sidebar snapshot/count projection
5. Prepare the backend read side for the runtime resource model introduced by `PBI-234` and `PBI-303`.

## Acceptance Criteria
1. Grid, selection, and sidebar read logic have clearer service boundaries.
2. Controllers become thin entry points rather than mixed query engines.
3. Shared scope/read semantics are consumed from one source, not redefined here.
4. Future runtime invalidation can target read services cleanly.

## Test Cases
1. Grid paging behavior remains unchanged.
2. Selection summaries still match current results.
3. Sidebar counts and structure remain correct.

## Risk
Medium-high. Large query surface with performance sensitivity.
