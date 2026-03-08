# PBI-352: Backend MutationImpact emitter purge

## Priority
P1

## Problem
The backend still emits many runtime mutations through `MutationImpact` presets that lean on `domains` fallback semantics. That keeps the old compatibility shape alive even after the runtime contract became fact-first.

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
