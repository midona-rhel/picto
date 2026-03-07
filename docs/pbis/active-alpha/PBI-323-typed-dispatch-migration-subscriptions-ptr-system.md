# PBI-323: Typed dispatch migration for subscriptions, PTR, and system commands

## Priority
P1

## Status
Not Implemented

## Problem
The long-running job and runtime-management domains still depend heavily on legacy string dispatch. These are exactly the commands where argument drift is most expensive because they coordinate background work, cancellation, bootstrap, and system actions.

## Scope
- `core/src/dispatch/subscriptions.rs`
- `core/src/dispatch/ptr.rs`
- `core/src/dispatch/system.rs`
- related frontend controllers

## Implementation
1. Add typed command structs for high-value subscription commands first.
2. Add typed command structs for PTR sync/bootstrap/status commands.
3. Add typed command structs for system/runtime commands that are safe to migrate.
4. Generate TS types.
5. Move frontend wrappers/controllers to typed invocations for migrated commands.
6. Remove migrated commands from legacy handlers.

## Acceptance Criteria
1. Subscription, PTR, and system commands in scope use typed dispatch.
2. Frontend controllers consume generated command types.
3. Runtime behavior is unchanged for start/stop/status/bootstrap flows.
4. Parity guard passes.

## Test Cases
1. Typed subscription run/pause/delete commands deserialize correctly.
2. Typed PTR sync/bootstrap commands deserialize correctly.
3. Invalid typed args fail with clear deserialization errors.
