# PBI-317: Runtime event system compatibility purge

## Priority
P1

## Problem
The backend currently carries both the new runtime communication model and the old invalidation/event model. That duplicates state ownership and keeps legacy alive.

## Scope
- `core/src/events.rs`
- runtime event/task emission paths
- legacy event-name families replaced by typed runtime receipts and task progress

## Implementation
1. Finish cutover to runtime mutation receipts and runtime task progress.
2. Remove legacy `Domain`, `Invalidate`, and `MutationImpact` compatibility layers.
3. Delete superseded legacy event names and emission helpers.
4. Keep one authoritative runtime snapshot/task registry path.

## Acceptance Criteria
1. `events.rs` only owns the canonical runtime event bus/emitter surface.
2. Legacy invalidation structures are removed.
3. Backend background jobs publish through one task-registry path.
4. Frontend consumers no longer depend on legacy event names.
