# PBI-325: Generate runtime contract types from Rust

## Priority
P1

## Status
Not Implemented

## Problem
Command types are now generated from Rust, but the runtime contract still has a manual TS mirror. That preserves drift risk in the event/task/snapshot protocol.

## Scope
- `core/src/runtime_contract/*`
- `src/types/generated/runtime-contract.ts`
- any build/codegen scripts needed

## Implementation
1. Export runtime contract types from Rust using the same codegen approach used for typed commands.
2. Generate the TS runtime-contract file from Rust.
3. Delete the manual mirror.
4. Add a parity/codegen guard so the runtime contract cannot silently drift.

## Acceptance Criteria
1. Runtime contract TS types are generated from Rust, not manually maintained.
2. Event/task/snapshot frontend consumers compile against generated types.
3. Manual runtime contract drift path is removed.

## Test Cases
1. Generated runtime contract files are produced successfully.
2. Frontend compiles against generated runtime contract types.
