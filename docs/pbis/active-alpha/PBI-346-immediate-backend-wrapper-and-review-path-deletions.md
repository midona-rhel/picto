# PBI-346: Immediate backend wrapper and review-path deletions

## Priority
P1

## Problem
Several backend files are thin compatibility shells or one-off leftovers that add layering without adding ownership.

## Scope
- `core/src/sidebar_controller.rs`
- `core/src/import_controller.rs`
- `core/src/view_prefs_controller.rs`
- `core/src/lifecycle_controller.rs`
- `core/src/dispatch/files_review.rs`

## Implementation
1. Move remaining call sites to the real owning services.
2. Fold review handlers into the files lifecycle dispatch path.
3. Delete the wrapper/legacy files.
4. Update `lib.rs` exports and module docs accordingly.

## Acceptance Criteria
1. None of the scoped files remain.
2. Behavior is unchanged.
3. No imports reference the removed paths.
4. Net backend LOC decreases.
