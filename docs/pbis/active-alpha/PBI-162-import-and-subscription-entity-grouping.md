# PBI-162: Group imports into collection entities for multi-image posts

## Priority
P0

## Audit Status (2026-03-03)
Status: **Blocked (Subscription Workstream Deferred)**

Blocked Reason:
1. Subscription workstream is deferred by product direction for now.
2. Keep this PBI in backlog but do not execute until unblocked.

## Problem
Import and subscription pipelines ingest media as isolated files. Multi-image posts require grouped entity creation to preserve post-level semantics and support collection UI.

## Scope
- `/Users/midona/Code/imaginator/core/src/subscriptions/*`
- `/Users/midona/Code/imaginator/core/src/import/*`
- `/Users/midona/Code/imaginator/core/src/state.rs`
- `/Users/midona/Code/imaginator/src/controllers/subscriptionController.ts`

## Implementation
1. Ingest grouping rule:
   - single-file posts -> create one `single` entity + file link.
   - multi-file posts (same source post id) -> create one `collection` entity + member singles.
2. Persist source identity fields for deterministic regrouping/idempotency.
3. Deduplicate at entity layer during ingest:
   - identical content can reuse blob rows.
   - entity creation follows grouping semantics.
4. Emit normalized mutation events so grid/sidebar refresh stays deterministic.

## Acceptance Criteria
1. Multi-image post import produces one collection with correctly ordered member singles.
2. Re-running import is idempotent (no duplicate collection explosion).
3. Single-image import behavior remains unchanged.

## Test Cases
1. Import gallery post with 10 images -> one collection entity + 10 members.
2. Import same post again -> no duplicate entities/members created.
3. Mixed import batch with singles and galleries -> both flows succeed in one run.

## Risk
High. Core ingest semantics and idempotency path change.

