# PBI-161: Rebind duplicates and PTR behavior to entity model

## Priority
P0

## Audit Status (2026-03-03)
Status: **Not Implemented**

## Problem
Current duplicate/PTR flows assume file-centric ownership and can incorrectly mix metadata across logically separate media entities and collections.

## Scope
- `/Users/midona/Code/imaginator/core/src/duplicates/*`
- `/Users/midona/Code/imaginator/core/src/ptr_*`
- `/Users/midona/Code/imaginator/src/components/duplicates/*`

## Implementation
1. Duplicate engine rules:
   - operate on `single` entities and their bound file blobs.
   - `single + single`: normal merge flow.
   - `single + collection-member`: member-level merge only; collection metadata untouched.
   - `collection + collection`: manual-only workflow, no auto destructive merge.
2. PTR overlay rules:
   - apply only to `single` entities.
   - collection can display derived read-only summary in detail view (no direct collection PTR state).
3. Prevent cross-kind metadata pollution:
   - collection tags never auto-propagate to member singles.
   - member single tags never auto-promote to collection without explicit user action.

## Acceptance Criteria
1. Duplicate decisions never implicitly rewrite unrelated collection metadata.
2. PTR tags appear correctly on singles and never incorrectly as writable collection state.
3. Existing duplicate UX remains functional for single-item workflows.

## Test Cases
1. Merge two standalone duplicates and verify expected metadata consolidation.
2. Resolve duplicate where one candidate is collection member; collection metadata remains stable.
3. PTR overlay lookup for mixed scope returns tags for singles only.

## Risk
Medium-high. Behavioral correctness in high-impact data mutation paths.

