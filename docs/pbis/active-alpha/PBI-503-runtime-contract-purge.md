# PBI-503: Runtime contract purge

## Priority
P0

## Problem
The runtime layer still carries compatibility shaping, duplicate task derivation, and domain-specific renderer logic that should already be encoded in the backend runtime contract.

## Goal
Make the frontend runtime model be snapshot + receipts + tasks, and nothing else.

## Implementation
1. Remove active use of domain-specific compatibility events.
2. Limit `runtimeSyncStore` to snapshot hydration, task registry projection, receipt-driven invalidation facts, and watchdog fallback.
3. Move domain-specific shaping out of the store.
4. Update runtime checks so they validate the actual contract.

## Acceptance Criteria
1. Frontend feature modules do not subscribe to their own runtime listeners.
2. Invalidations derive from mutation facts.
3. `runtimeSyncStore` no longer owns broad domain orchestration.
