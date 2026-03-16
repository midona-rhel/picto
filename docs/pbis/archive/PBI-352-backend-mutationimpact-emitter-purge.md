# PBI-352: Backend MutationImpact emitter purge

## Priority
P1

## Audit Status (2026-03-15)
Status: **Implemented**

## Problem
The backend still emits many runtime mutations through `MutationImpact` presets that lean on `domains` fallback semantics. That keeps the old compatibility shape alive even after the runtime contract became fact-first.

Evidence:
1. Broad fallback presets `all_domains_change()` and `domain_only()` were removed from active use and deleted from `core/src/events.rs`.
2. Collection membership and reorder emitters now publish explicit `folder_membership_changed`, `file_hashes`, and `extra_grid_scopes` facts instead of pretending to be `status_changed` or `Domain::Files` only.
3. Duplicate resolution now emits from `core/src/duplicates/controller.rs`, which has the winner/loser/folder facts, instead of a generic dispatch-layer placeholder.
4. Existing-file merge receipts in `core/src/import/existing.rs` now distinguish status restoration, tag changes, metadata changes, and subscription ownership changes instead of collapsing them into `file_lifecycle`.
5. `compiler_publish()` remains only as a minimal transitional builder for sidebar tree publication, not as a broad semantic preset.

## Scope
- `core/src/events.rs`
- backend emitter call sites using `MutationImpact`
- worker/compiler mutation emission paths

## Implementation
1. Audit `MutationImpact` presets and identify which are still domain-fallback driven.
2. Replace backend emitters that can express their change through explicit fact fields alone.
3. Keep temporary compatibility only where the frontend still genuinely lacks explicit fact coverage.
4. Reduce `Domain` usage to cases that are still intentionally transitional.

## Acceptance Criteria
1. New backend emitters do not rely on domain-only mutation receipts.
2. Existing backend emitters in the touched slice prefer explicit facts over `domains` fallback.
3. `MutationImpact` remains only as a transitional builder, not the semantic source of truth.
4. The diff does not require frontend changes to remain correct.

## Verification
1. Trigger the touched backend mutation paths.
2. Confirm the emitted receipts still carry the expected file/folder/tag facts.
3. Confirm existing frontend behavior does not regress, since compatibility is preserved where still needed.

## Risk
Medium. This changes mutation emission shape in correctness-sensitive paths.
