# PBI-234: Typed dispatch contract between core and frontend

## Priority
P1

## Audit Status (2026-03-07)
Status: **Implemented**

All 12 command domain families are fully migrated to typed dispatch:

1. `core/src/dispatch/typed/` — `TypedCommand` trait with compile-time checked `Input`/`Output` types
2. Domain modules: `media_lifecycle`, `media_metadata`, `media_io`, `folders`, `tags`, `selection`, `grid`, `subscriptions`, `ptr`, `system`, `duplicates`, `smart_folders`
3. `dispatch/mod.rs` — all commands route through `typed::typed_dispatch()`. Two pre-state commands (`close_library`, `get_runtime_snapshot`) are handled inline because they don't require `AppState`.
4. Frontend `invokeTyped()` — compile-time checked command names and arg types via generated `TypedCommandMap`
5. No remaining legacy string-based command dispatch — unknown commands return an error

## Problem
The dispatch layer has no type safety. Command names are strings, arguments are untyped JSON, and there is no shared contract between the Rust core and the TypeScript frontend. This makes it impossible to know at compile time whether a command call is correct, creates drift between frontend and backend, and means every new command requires manually matching string names and JSON shapes in both codebases.

## Scope
- `core/src/dispatch/` — all domain handler modules
- `native/picto-node/src/lib.rs` — NAPI bridge
- Frontend command invocation layer

## Implementation
1. Define a Rust enum of all commands with typed argument and return structs (e.g. `enum Command { UpdateFileStatus { hash: String, status: String }, ... }`).
2. Use serde to derive serialization for all command/response types.
3. Generate TypeScript types from the Rust definitions (via `ts-rs`, `specta`, or a build-time codegen script).
4. Replace the stringly-typed `invoke(command, argsJson)` NAPI function with a typed wrapper that validates against the schema.
5. Phase in incrementally — can coexist with the old string dispatch during migration.

## Acceptance Criteria
1. At least one domain (e.g. files_lifecycle) uses typed command structs.
2. TypeScript types are generated from Rust definitions — frontend gets compile-time checking.
3. Invalid command arguments produce clear error messages at deserialization, not silent fallthrough.
4. No runtime behavior change — existing commands continue to work.

## Test Cases
1. Call a typed command with correct arguments — succeeds.
2. Call a typed command with wrong argument type — returns clear deserialization error.
3. Generated TypeScript types match Rust structs.
4. Old string-based dispatch still works for non-migrated commands.

## Risk
High. Touches every dispatch path. Must be phased — migrate one domain at a time. Consider `specta` or `ts-rs` for type generation.
