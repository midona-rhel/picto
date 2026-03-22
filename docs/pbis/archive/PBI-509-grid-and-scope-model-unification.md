# PBI-509: Grid and scope model unification

## Status
Implemented and archived.

## What was fixed
1. Grid and selection now share one canonical query contract:
   - `scope`
   - `filters`
   - `sort`
2. Backend scope resolution is the single source of truth for:
   - system scopes
   - folder scopes
   - collection scopes
   - smart-folder scopes
3. Renderer grid state, selection, export, viewer detail-window feeds, and refresh matching were moved onto that same scope model.
4. `system:active` is the canonical root library scope.
   - `system:active_files` remains backend read-side migration tolerance only.
5. `random` is treated as ordering over the canonical `system:active` dataset, not a separate dataset.
6. Grid `total_count` is now the renderer count authority.
   - selection/export/detail flows no longer fall back to sidebar-derived counts.

## Delivered slices
1. Canonical backend grid scope contract
2. Renderer adoption of the canonical nested grid/selection contract
3. Grid totals as the count authority
4. Root scope key normalization in the renderer

## Acceptance result
1. No active parallel grid query shape remains between backend grid and selection flows.
2. Scope behavior is centralized around backend scope resolution plus shared renderer scope helpers.
3. Grouped collection-member visibility and collection scope semantics stay consistent across grid, selection, export, and viewer flows.

## Remaining non-goals
1. Sidebar/navigation read-model cleanup belongs to `PBI-510`.
2. Inspector/metadata DTO consolidation belongs to `PBI-511`.
3. Backend migration tolerance for legacy `system:active_files` rows remains intentional.
