# PBI-300: Canonical Scope Semantics Engine

## Priority
P0

## Audit Status (2026-03-07)
Status: **Implemented**

### What's done:
1. `core/src/scope/mod.rs` — shared `IncludeMatchMode` enum and `parse_include_match_mode` parser.
2. `core/src/scope/resolver.rs` — canonical `ScopeFilter` struct with `From` impls for both `GridPageSlimQuery` and `SelectionQuerySpec`. Single `resolve_scope()` function handles all scope types: smart folder, tag search, folder, untagged, uncategorized, recently_viewed, inbox, trash, default (active). Synchronous `scope_count()` function provides canonical counts for system scopes.
3. `core/src/grid/controller.rs` — three separate scope resolution blocks (~350 lines) collapsed into one unified block calling `resolve_scope`. Grid-specific concerns (caching, pagination, single-folder position_rank ordering, collection, recently_viewed, random) remain in the grid controller.
4. `core/src/selection/helpers.rs` — `selection_bitmap_for_all_results` reduced from ~170 lines to ~15 lines by delegating to `resolve_scope`. Selection-specific concerns (excluded_hashes filtering) remain.
5. Duplicate `IncludeMatchMode` enums and `parse_include_match_mode` functions removed from both files.
6. `core/src/sqlite/compilers.rs` — sidebar system counts (all, inbox, trash, untagged, uncategorized, recently_viewed) now delegate to `scope_count()` instead of inline bitmap/SQL logic. `count_uncategorized_entities` import removed from compilers.

## Problem
Scope semantics were previously duplicated across multiple backend paths:

1. `core/src/grid/controller.rs`
2. `core/src/selection/helpers.rs`

This caused business-logic drift risk. The same conceptual scope:
- `system:all`
- `system:inbox`
- `system:untagged`
- `system:uncategorized`
- tag search scopes
- folder scopes

can behave differently depending on whether it is used for:
- grid paging
- select-all
- selection summary
- count calculation

That is an architecture bug, not a documentation problem.

## Goal
Create one canonical backend scope engine that defines visible entity membership once and is reused everywhere.

The backend must stop re-implementing scope rules independently per caller.

## Scope
- `core/src/scope/` — canonical scope resolver module
- `core/src/grid/controller.rs` — grid page queries (consumer)
- `core/src/selection/helpers.rs` — selection bitmap operations (consumer)

## Implementation
1. Introduce a canonical scope model:
   - `ScopeSpec`
   - `ResolvedScope`
   - explicit variants for status, folder, smart folder, tag search, color search, uncategorized, untagged, recently viewed, random
2. Add a single scope resolver service that:
   - resolves the base entity bitmap for a scope
   - applies status visibility rules
   - applies include/exclude tags and folders
   - applies smart-folder predicates
   - returns one reusable `ResolvedScope`
3. Move `selection_bitmap_for_all_results()` to consume the same scope resolver rather than re-deriving rules.
4. Make grid paging consume the same scope resolver rather than inlining separate status/folder/tag behavior.
5. Expose one helper for scope count computation so sidebar counts and grid totals are based on the same semantics.
6. Remove duplicate comments describing scope behavior from multiple locations once semantics are centralized.

## Explicit Business Rules To Codify
1. `system:all` means active only unless product explicitly says otherwise.
2. `system:inbox` means tentative/unreviewed only.
3. Trash is excluded from all other scopes unless explicitly requested.
4. `untagged` means active items with no effective tags.
5. `uncategorized` means active items with no folder membership.
6. `select all` means all currently visible items in the resolved scope, not a second interpretation of the scope.
7. Multiple folder filters use union semantics unless explicitly configured otherwise.
8. Multiple tag filters use intersection semantics unless explicitly configured otherwise.

## Acceptance Criteria
1. Grid paging and select-all use the same scope resolution path.
2. There is exactly one canonical place in the backend that defines the business meaning of each scope.
3. Sidebar counts and selection summaries do not re-implement scope visibility rules separately.
4. Comments do not describe contradictory scope behavior in different files.
5. `system:all`, `untagged`, `uncategorized`, tag search, and folder search have one authoritative implementation.

## Test Cases
1. `system:all` grid and select-all include the same entity set.
2. `system:inbox` grid and select-all include the same entity set.
3. Empty folder and uncategorized counts remain correct after status changes.
4. Tag search with two tags yields the same population for:
   - grid page
   - select all
   - selection summary
5. Folder union/exclusion logic matches between grid and selection.

## Risk
High. This touches core read semantics and will surface latent business-rule disagreements. That is expected and desirable.
