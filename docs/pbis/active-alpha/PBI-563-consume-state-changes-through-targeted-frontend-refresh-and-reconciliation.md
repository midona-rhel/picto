# PBI-563: Consume State Changes Through Targeted Frontend Refresh And Reconciliation

## AI-Generated Caveat
This PBI is AI-generated and assumes the backend contract work from PBI-560 through PBI-562 is either complete or far enough along to consume richer payloads. If it is not, this PBI should be blocked until the event payload is strong enough.

## Priority
P0

## Problem
Even with better state-change events, the frontend can still waste work if it keeps relying on broad fallback refresh patterns rather than targeted refresh and reconciliation.

## Goal
Consume `runtime/state_changed` through targeted frontend refresh planning and explicit reconciliation of eager controller updates.

## Atomicity Rule
This PBI should focus on runtime consumption and reconciliation. Do not reopen controller ownership unless the runtime layer cannot function without a small change.

## Scope
- [src/runtime/stateChanges/stateChangeStore.ts](./src/runtime/stateChanges/stateChangeStore.ts)
- [src/runtime/stateChanges/planRefreshTargets.ts](./src/runtime/stateChanges/planRefreshTargets.ts)
- [src/runtime/refresherOrchestrator.ts](./src/runtime/refresherOrchestrator.ts)
- relevant runtime tests under `src/runtime/**/__tests__`

## Required Outcome
1. Rich backend deltas map to targeted refresh work.
2. Broad fallback invalidation is reduced to true fallback cases, not normal behavior.
3. Controller eager updates reconcile cleanly when authoritative backend state arrives.

## Required Targeting
Use exact:

- file hashes
- folder ids
- collection ids
- smart folder ids
- subscription ids where relevant
- changed fields
- changed scopes

## Look For Adjacent Improvements
- remove stale terminology that still implies vague invalidation
- collapse duplicate refresh-target planning logic
- simplify applier ordering if it is too coupled to old broad refresh behavior
- collapse near-duplicate refresh/reconciliation paths that differ only by trivial wrapper behavior
- rename refresh/reconciliation helpers so they describe function, not implementation noise

## Non-Goals
- redefining the backend contract itself
- redoing all domain controllers

## Acceptance Criteria
1. Representative state changes refresh only the affected read models.
2. Broad “refresh all” behavior is no longer a correctness requirement for normal flows.
3. Runtime tests cover mapping from rich deltas to targeted refreshes.

## Validation
- runtime unit tests
- manual checks on files/tags/folders/subscriptions representative flows
