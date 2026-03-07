# PBI-326: Remove legacy dispatch paths for migrated commands

## Priority
P1

## Status
Not Implemented

## Problem
A phased migration only pays off if migrated commands are actually removed from the legacy dispatch path. Otherwise the system keeps two homes for the same behavior and the dispatch layer remains ambiguous.

## Scope
- legacy dispatch match arms for commands that already have typed implementations
- parity/guard scripts
- command wrapper cleanup in frontend if needed

## Implementation
1. For each migrated command, delete the legacy handler arm.
2. Strengthen the typed parity checker to fail on any migrated-command duplication.
3. Remove dead helper code made obsolete by legacy handler deletion.
4. Keep the non-migrated commands on legacy dispatch until their own migration PBIs land.

## Acceptance Criteria
1. No migrated command is handled in both typed and legacy dispatch.
2. Parity guard fails if duplication is reintroduced.
3. Dispatch ownership for migrated commands is unambiguous.

## Test Cases
1. Typed parity checker passes.
2. Migrated commands still function through typed dispatch only.
