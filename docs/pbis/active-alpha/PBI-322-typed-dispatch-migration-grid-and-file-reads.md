# PBI-322: Typed dispatch migration for grid and file read commands

## Priority
P1

## Status
Not Implemented

## Problem
After lifecycle, the most frequently invoked read paths are still on legacy string dispatch: grid paging, metadata batch reads, and media/read helpers. These should move to typed dispatch so the highest-volume renderer calls stop depending on raw JSON convention.

## Scope
- `core/src/dispatch/grid.rs`
- `core/src/dispatch/files_metadata.rs`
- `core/src/dispatch/files_media.rs`
- relevant frontend read-path invocations

## Implementation
1. Add typed command structs for grid page fetch and related grid reads.
2. Add typed command structs for metadata batch and single-file metadata reads.
3. Add typed command structs for media-oriented read helpers where practical.
4. Generate TS types.
5. Move frontend call sites for migrated reads onto typed command signatures.
6. Remove migrated commands from legacy match arms.

## Acceptance Criteria
1. Grid and metadata read commands in scope route through typed dispatch.
2. Frontend read-path wrappers use generated types for migrated commands.
3. No runtime behavior regression on paging or metadata fetch.
4. Parity guard passes.

## Test Cases
1. Typed grid page command succeeds with valid input.
2. Invalid typed read args fail at deserialization with clear errors.
3. Metadata batch types match Rust-generated schema.
