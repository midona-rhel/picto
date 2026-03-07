# PBI-328: Fact-Based Mutation Receipts And Model-Level Invalidation

## Priority
P0

## Audit Status (2026-03-07)
Status: **Not Implemented**

## Problem
Mutation communication is still too close to UI invalidation and too far from model truth.

Current problems:
1. The backend still reasons in terms of broad invalidation hints instead of explicit changed facts.
2. Mutation meaning is partly encoded by:
   - handler-specific `MutationImpact` construction
   - compiler side-effects
   - frontend resource invalidation heuristics
3. The frontend is forced to infer too much from partial invalidation signals.

This is why operations like:
- move image into auto-tagged folder
- restore from trash
- merge duplicates
- subscription import

can still require special-case refresh behavior.

## Goal
Make backend mutation communication model-first:
1. backend emits structured facts about what changed
2. resource invalidation is derived from those facts
3. the backend does not speak in UI-specific terms as the primary contract

## Scope
- `/Users/midona/Code/imaginator/core/src/events.rs`
- `/Users/midona/Code/imaginator/core/src/runtime_contract/mutation.rs`
- mutation-emitting typed handlers under `/Users/midona/Code/imaginator/core/src/dispatch/typed/`
- controllers that emit mutations indirectly

## Implementation
1. Expand the runtime mutation contract to carry explicit facts such as:
   - `entity_ids_changed`
   - `entity_status_changes`
   - `entity_tag_changes`
   - `folder_ids_changed`
   - `folder_membership_changes`
   - `smart_folder_ids_changed`
   - `view_prefs_changed`
2. Reduce `MutationImpact` to a backend fact builder, not a UI invalidation bag.
3. Separate:
   - mutation facts
   - derived resource invalidation
   - task progress
4. Ensure every mutating command emits facts that are sufficient to explain:
   - what records changed
   - what relationships changed
   - which domains changed
5. Remove mutation payload fields that exist only as legacy UI hints once the fact model is adopted.

## Explicit Non-Goals
1. Backend does not emit “refresh inspector” or “refresh sidebar panel”.
2. Backend does not encode view-specific refresh semantics as the source of truth.

## Acceptance Criteria
1. Every mutating command emits a fact-based receipt.
2. Receipts can explain folder membership changes, tag changes, and status changes without UI-specific interpretation.
3. Subscription imports, duplicate resolution, and folder auto-tagging emit fact-complete receipts.
4. The frontend can determine stale resources from facts alone.
5. Legacy invalidation-only mutation thinking is materially reduced.

## Test Cases
1. Move file into auto-tagged folder:
   - receipt includes folder membership change and tag change
2. Restore file from trash:
   - receipt includes status transition and affected entity id
3. Duplicate merge:
   - receipt includes changed winner/loser entity facts
4. Subscription import:
   - receipt includes new entity ids and any tag/folder/status facts

## Risk
High. This changes the core semantics of backend/frontend coordination and will expose missing fact ownership in multiple domains.
