# Dispatch Domain

## Purpose

The dispatch layer routes command names from the frontend (via napi-rs IPC) to typed domain handlers. It provides compile-time type safety for command arguments and return values.

## Routing Model

```
Frontend → napi addon → dispatch(command, args_json)
                            │
                            ├── "close_library"     → inline (no state needed)
                            ├── "get_runtime_snapshot" → inline (stateless)
                            │
                            └── typed_dispatch(state, command, args)
                                  │
                                  ├── media_lifecycle::dispatch_typed()
                                  ├── folders::dispatch_typed()
                                  ├── tags::dispatch_typed()
                                  ├── selection::dispatch_typed()
                                  ├── grid::dispatch_typed()
                                  ├── media_metadata::dispatch_typed()
                                  ├── media_io::dispatch_typed()
                                  ├── subscriptions::dispatch_typed()
                                  ├── ptr::dispatch_typed()
                                  ├── system::dispatch_typed()
                                  ├── duplicates::dispatch_typed()
                                  └── smart_folders::dispatch_typed()
```

Each domain module's `dispatch_typed()` returns `Option<Result<String, String>>`:
- `Some(Ok(json))` — command matched and succeeded
- `Some(Err(msg))` — command matched but failed
- `None` — command not handled, fall through to next module

Routing order is not semantically significant — each command name is unique across all modules.

## TypedCommand Trait

Every command is a zero-sized struct implementing `TypedCommand`:

```rust
trait TypedCommand {
    const NAME: &'static str;           // IPC command string (e.g. "import_files")
    type Input: DeserializeOwned;       // Deserialized from JSON args
    type Output: Serialize;             // Serialized to JSON result
    fn execute(state, input) -> Result<Output, String>;
}
```

The `run_typed::<C>()` helper deserializes args, calls execute, serializes output.

## Command Naming Convention

Command names are IPC contract strings shared between Rust and TypeScript. They use `snake_case` (e.g. `import_files`, `update_file_status`, `search_tags`). These are NOT Rust module names — the IPC strings are stable and must not be renamed without updating the TypeScript side.

## Contracts

- Every command handler returns `Result<String, String>` where the `String` is JSON.
- Mutation commands must construct a `MutationImpact` and call `emit_mutation()` to notify the frontend.
- Read-only queries (grid, metadata, sidebar) just return data without emitting mutations.

## Key Files

- `core/src/dispatch/mod.rs` — top-level dispatch entry point
- `core/src/dispatch/common.rs` — JSON helpers (`ok_null`, `to_json`)
- `core/src/dispatch/typed/mod.rs` — `TypedCommand` trait, `typed_dispatch` router
- `core/src/dispatch/typed/*.rs` — per-domain command implementations
