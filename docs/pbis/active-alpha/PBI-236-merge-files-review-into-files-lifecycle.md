# PBI-236: Merge files_review into files_lifecycle

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `dispatch/files_review.rs` contains 3 commands: `get_review_queue`, `review_image_action`, `get_review_item_image`.
2. `review_image_action` is just a thin wrapper around `LifecycleController::update_file_status` — it maps "approve"→1 and "reject"→2, then constructs the same MutationImpact as `update_file_status` in files_lifecycle.
3. `get_review_queue` is a hardcoded `list_files_slim(50, Some(0), ...)` — a specialized grid query that could be a parameterized grid command.
4. `get_review_item_image` reads the original blob — identical to logic in `files_media.rs`.

## Problem
The `files_review` dispatch module creates a false domain boundary. "Review" is not a separate domain — it's just file status management (inbox → active or inbox → trash) with a specialized view. Having it as a separate module:
- Duplicates MutationImpact construction
- Creates confusion about which command to use for status changes
- Makes it unclear where to add new inbox-related features

## Scope
- `core/src/dispatch/files_review.rs` — to be removed
- `core/src/dispatch/files_lifecycle.rs` — absorb review commands
- `core/src/dispatch/files.rs` — remove files_review routing
- `core/src/dispatch/mod.rs` — remove files_review module declaration

## Implementation
1. Move `get_review_queue` to files_lifecycle (or files_metadata as a specialized query).
2. Replace `review_image_action` with calls to the existing `update_file_status` command, adding an alias if needed for backward compatibility.
3. Move `get_review_item_image` to files_media (it's just blob reading).
4. Delete `files_review.rs`.
5. Update frontend callers if command names change.

## Acceptance Criteria
1. `files_review.rs` is deleted.
2. All three commands still work (either natively or via aliases).
3. No behavior change for the frontend.
4. MutationImpact duplication is reduced.

## Test Cases
1. Review queue loads correctly after merge.
2. Approve/reject from inbox works.
3. Review image display works.

## Risk
Low. Small module, clear 1:1 mapping to existing lifecycle/media commands.
