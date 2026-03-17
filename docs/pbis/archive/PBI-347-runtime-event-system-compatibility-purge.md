# PBI-347: Runtime event system compatibility purge

## Priority
P1

## Status
Implemented

## Problem
The backend still carries compatibility scaffolding from the old invalidation/event model even though the runtime contract and task registry now exist. The remaining issue is no longer “build the runtime bus”; it is “purge backend compatibility layers without mixing that work into frontend resource-derivation changes.”

## Scope
- `core/src/events.rs`
- runtime mutation/task emission paths
- backend compatibility ownership only
- `MutationImpact`, `Domain`, and domain-only compatibility emission patterns

## Current State
Already done:
1. Canonical runtime event names exist (`runtime/mutation_committed`, `runtime/task_upserted`, `runtime/task_removed`).
2. Runtime task registry exists.
3. Runtime contract types exist under `core/src/runtime_contract/`.
4. `Domain` ownership has started moving out of `events.rs`.

Still open on the backend:
1. `MutationImpact` remains the compatibility builder used by many emitters.
2. Many emitters still rely on `facts.domains` fallback semantics instead of explicit model facts alone.
3. Compiler/worker paths still manufacture compatibility-oriented mutation shapes.

Explicitly out of scope here:
1. Frontend removal of `facts.domains` fallback logic.
2. Renderer-side resource invalidator cleanup.

## Implementation
1. Finish backend ownership cutover so runtime contract types are owned by `runtime_contract/*`, not `events.rs`.
2. Remove backend-only legacy compatibility layers incrementally.
3. Replace domain-only compatibility emissions with explicit fact fields where possible.
4. Keep one authoritative runtime snapshot/task registry path on the backend.

## Acceptance Criteria
1. `events.rs` owns only event transport/emitter behavior.
2. Runtime contract types are no longer owned by `events.rs`.
3. Backend background jobs publish through one task-registry path.
4. Remaining compatibility fallbacks are explicitly frontend-bound follow-up work, not hidden backend scaffolding in this ticket.

## Completion Notes
1. `MutationImpact` and its helpers now live under `core/src/runtime_contract/`.
2. `events.rs` is transport-only.
3. Redundant backend `facts.domains` fallbacks were pruned where explicit facts already drive invalidation.
4. Compiler publish no longer carries the dead backend-only `sidebar_affected` compatibility flag.
5. Renderer-side `facts.domains` fallback remains out of scope for this ticket and belongs to the frontend/runtime-contract cleanup PBIs.
