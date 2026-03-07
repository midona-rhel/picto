# PBI-303: Derived Resource Dependency Map From Model Facts

## Priority
P0

## Audit Status (2026-03-07)
Status: **Not Implemented**

## Problem
The system currently treats invalidation as if the UI is the model:
- grid invalidated
- sidebar invalidated
- inspector stale

That is backwards.

The real dependency chain is:
1. model facts change
2. derived resources become stale
3. UI surfaces re-read those resources

Without an explicit dependency map, refresh behavior remains ad hoc and feature-specific.

## Goal
Define and implement one backend-owned dependency map from model facts to derived resources.

This does not mean the backend should manage React components. It means backend/runtime contracts should make resource staleness derivable from changed facts in a deterministic way.

## Scope
- `/Users/midona/Code/imaginator/core/src/runtime_contract/mutation.rs`
- `/Users/midona/Code/imaginator/core/src/events.rs`
- runtime/resource invalidation consumers on the renderer side
- read resources for:
  - grid scopes
  - metadata
  - sidebar snapshot/counts
  - selection summaries

## Implementation
1. Define canonical derived resource keys:
   - `grid:<scope_key>`
   - `metadata:<entity_id>`
   - `sidebar:snapshot`
   - `sidebar:counts`
   - `selection:<selection_key>`
2. For each mutation fact type, define deterministic invalidation rules.
Examples:
   - entity status change -> metadata entity + all status-sensitive scopes + sidebar counts
   - folder membership change -> folder grids + uncategorized + metadata entity + sidebar snapshot
   - tag change -> metadata entity + tag-sensitive scopes + untagged + smart-folder resources
3. Remove broad fallback invalidations where a precise dependency rule can be used.
4. Document the dependency map as a contract, not as implementation folklore.

## Acceptance Criteria
1. Resource invalidation is described in terms of derived resources, not UI panels.
2. A single mutation fact type always yields the same stale-resource set.
3. Grid/resource invalidation no longer depends on ad hoc controller-specific refresh logic.
4. The invalidation rules are documented and testable.

## Test Cases
1. Tagging a file invalidates:
   - that file’s metadata resource
   - any tag-sensitive grid scope
   - untagged if the file transitioned from untagged to tagged
2. Folder membership change invalidates:
   - the folder resource
   - uncategorized if membership crossed zero/non-zero
   - metadata for the affected entity
3. Status change invalidates:
   - old/new status scopes
   - sidebar counts
   - metadata for the affected entity

## Risk
Medium-high. It is easy to keep broad invalidation and claim success. This PBI only counts as done if the rules become explicit and deterministic.
