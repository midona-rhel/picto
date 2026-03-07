# PBI-324: Frontend typed invoke adoption for migrated commands

## Priority
P1

## Status
Not Implemented

## Problem
The typed dispatch infrastructure now exists, but the frontend mostly still uses the generic `invoke()` wrappers. That means the compile-time safety benefits are only partially realized.

## Scope
- `src/desktop/api.ts`
- frontend controllers and call sites for commands already migrated to typed dispatch

## Implementation
1. Move migrated command wrappers to `invokeTyped()`.
2. Ensure controllers import generated command types where appropriate.
3. Remove untyped wrappers for commands that are now fully migrated.
4. Add guardrails to prevent reintroducing generic wrappers for migrated commands.

## Acceptance Criteria
1. Frontend uses `invokeTyped()` for all commands that have a typed backend implementation.
2. Call sites receive compile-time checking from generated types.
3. There is no duplicate typed/untyped wrapper surface for the same migrated command.

## Test Cases
1. Frontend build fails if a typed command is called with wrong input shape.
2. Migrated call sites continue to function at runtime.
