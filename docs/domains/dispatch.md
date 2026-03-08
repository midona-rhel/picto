# Dispatch Domain

## Purpose

The dispatch layer routes command names from the frontend (via napi-rs IPC) to typed handler functions.

## Routing Model

```
Frontend → napi addon → dispatch(command, args_json)
                            │
                            ├── "close_library"        → inline (no state needed)
                            ├── "get_runtime_snapshot"  → inline (stateless)
                            │
                            └── flat match on command name
                                  → call!(typed::{domain}::{handler}, &state, args)
```

`dispatch/mod.rs` contains a single flat `match command { ... }` with one arm per command (~142 commands). The `call!()` macro deserializes the JSON args into the handler's input struct, calls the async handler, and serializes the output.

## Handler Functions

Each domain has a file in `dispatch/typed/` containing plain async functions:

```rust
pub async fn some_command(state: &AppState, input: SomeInput) -> Result<SomeOutput, String> { ... }
```

Input structs derive `Deserialize` + `TS` for automatic TypeScript binding generation.

## Command Naming Convention

Command names are IPC contract strings shared between Rust and TypeScript. They use `snake_case` (e.g. `import_files`, `update_file_status`). These are stable and must not be renamed without updating the TypeScript side (`TypedCommandMap` in `commands/index.ts`).

## Contracts

- Every handler returns `Result<T, String>` where T is serialized to JSON by `call!()`.
- Mutation commands construct a `MutationImpact` and call `emit_mutation()` to notify the frontend.
- Read-only queries just return data without emitting mutations.

## Key Files

- `core/src/dispatch/mod.rs` — flat command match + `call!()` macro
- `core/src/dispatch/common.rs` — JSON helpers (`ok_null`, `to_json`)
- `core/src/dispatch/typed/mod.rs` — module declarations
- `core/src/dispatch/typed/*.rs` — per-domain handler functions
