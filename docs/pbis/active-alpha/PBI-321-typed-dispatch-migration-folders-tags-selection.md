# PBI-321: Typed dispatch migration for folders, tags, and selection

## Priority
P1

## Status
Not Implemented

## Problem
`PBI-234` now has a working typed vertical slice for file lifecycle commands, but the next interactive domains still run through legacy string dispatch. Folder, tag, and selection mutations are frequent, state-sensitive operations and are the next best candidates for typed migration.

## Scope
- `core/src/dispatch/folders.rs`
- `core/src/dispatch/tags.rs`
- `core/src/dispatch/selection.rs`
- new typed modules under `core/src/dispatch/typed/`
- generated TS types for migrated commands

## Implementation
1. Add typed command structs for the folder commands with the highest mutation impact first.
2. Add typed command structs for tag add/remove and tag-search operations that are safe to migrate.
3. Add typed command structs for selection mutation and summary operations.
4. Generate TS command types via `ts-rs`.
5. Remove migrated commands from legacy match arms.
6. Extend typed command parity checks.

## Acceptance Criteria
1. Folder, tag, and selection commands covered by this PBI route through typed dispatch.
2. Generated TS types exist for all migrated commands.
3. Legacy dispatch no longer handles those migrated command names.
4. Existing runtime behavior is preserved.

## Test Cases
1. Typed folder update/create/delete commands deserialize correctly.
2. Typed tag add/remove commands deserialize correctly.
3. Typed selection commands deserialize correctly.
4. Parity guard passes.
