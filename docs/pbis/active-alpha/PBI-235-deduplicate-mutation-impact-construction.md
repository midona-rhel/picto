# PBI-235: Deduplicate MutationImpact construction across dispatch handlers

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `files_lifecycle.rs` constructs nearly identical `MutationImpact` blocks in 6+ handlers (update_file_status, delete_file, delete_files, delete_files_selection, update_file_status_selection, wipe_image_data).
2. `files_review.rs` copies the same pattern for `review_image_action`.
3. Each block manually specifies the same 5 domains, sidebar_tree, selection_summary, grid_scopes, and sidebar_counts — only differing in file_hashes and folder_ids.
4. A change to invalidation logic (e.g. adding a new domain) requires touching every handler individually.

## Problem
MutationImpact construction is copy-pasted across dispatch handlers. The same invalidation pattern (domains, sidebar, selection, grid scopes, counts) is repeated with minor variations, making it easy to forget a domain or introduce inconsistency. The existing convenience constructors (`file_lifecycle()`, `file_metadata()`, `file_tags()`) cover only a few cases.

## Scope
- `core/src/events.rs` — `MutationImpact` convenience constructors
- `core/src/dispatch/files_lifecycle.rs` — 6+ handlers
- `core/src/dispatch/files_review.rs` — 1 handler
- `core/src/dispatch/tags.rs` — multiple handlers

## Implementation
1. Audit every `MutationImpact::new()` call site and categorize into patterns:
   - **file_status_change**: domains(Files, Sidebar, Folders, SmartFolders, Selection) + sidebar_tree + selection_summary + grid_scopes(status scopes) + sidebar_counts + file_hashes + folder_ids
   - **file_delete**: same as status_change but with grid_all
   - **tag_mutation**: domains(Tags, Files) + metadata_hashes + file_hashes
   - **sidebar_structure_change**: domains(domain, Sidebar) + sidebar_tree
2. Add named constructors on `MutationImpact` for each pattern, parameterized by the varying parts (hashes, folder_ids, scopes).
3. Replace all duplicated blocks with the named constructors.
4. Add doc comments explaining when each constructor should be used.

## Acceptance Criteria
1. No dispatch handler manually constructs a MutationImpact with more than 2-3 builder calls.
2. Common patterns are captured in named constructors with clear documentation.
3. All existing event emission behavior is preserved (no invalidation regressions).
4. Adding a new domain to a pattern requires changing one constructor, not N handlers.

## Test Cases
1. Existing tests pass unchanged.
2. `update_file_status` and `delete_file` emit identical domain sets as before.
3. New handler using a named constructor emits correct impact without manual domain listing.

## Risk
Low. Pure refactor of event construction — no behavior change. Can be done handler-by-handler.
