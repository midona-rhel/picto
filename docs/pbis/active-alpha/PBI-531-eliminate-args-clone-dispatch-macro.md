# PBI-531: Eliminate unnecessary args clone in dispatch macro

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The performance impact depends on payload sizes. For most commands (small JSON objects), the clone cost is negligible. For batch operations (e.g., `get_entities_metadata_batch` with hundreds of hashes), the clone doubles memory usage for the args payload.

## Priority
P2

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: In `core/src/dispatch/mod.rs` line 14, the `call!` macro clones the entire `serde_json::Value` before deserializing:
```rust
let input = serde_json::from_value($args.clone())
```
`serde_json::from_value()` takes ownership via move — the clone is unnecessary when the args value is not used after deserialization. Since `dispatch_inner()` receives `args: serde_json::Value` by value (not reference), the args can be moved directly into `from_value()`.

## Problem
The `call!` macro clones the `serde_json::Value` args on every command dispatch before deserializing. Since `from_value()` takes ownership, the original value is never used after the clone. This is a redundant deep copy.

For small payloads this is negligible, but for batch commands (e.g., `get_entities_metadata_batch` with 100+ hashes, `import_files` with file lists, `add_tags_selection` with large selections), this doubles the memory allocation for the args payload on every call.

## Scope
- `core/src/dispatch/mod.rs` — `call!` macro definition (line 12-18)

## Implementation
1. **Change the macro** to move instead of clone:
   ```rust
   macro_rules! call {
       ($func:path, $state:expr, $args:expr) => {{
           let input = serde_json::from_value($args)
               .map_err(|e| format!("Invalid args: {e}"))?;
           let output = $func($state, input).await?;
           to_json(&output)
       }};
   }
   ```
2. **Verify the match arms**: Each arm in `dispatch_inner` uses `args` only once via the `call!` macro. Since `dispatch_inner` receives `args` by value, each match arm can consume it.
3. **Compile and test**: `cargo check --manifest-path core/Cargo.toml` should pass without changes to any handler.

## Acceptance Criteria
1. The `call!` macro uses `$args` by move, not `$args.clone()`.
2. `cargo check` passes with no additional changes.
3. `cargo test` passes (all dispatch tests still work).
4. No behavioral change — same JSON in, same JSON out.

## Test Cases
1. All existing dispatch tests pass unchanged.
2. `get_entities_metadata_batch` with 200 hashes — verify no regression in response.
3. `import_files` with a list of 50 paths — verify no regression.

## Risk
Low. This is a one-line change with clear semantics. The only risk is if any match arm somehow uses `args` after the `call!` macro, which would cause a compile error (move after use) — the compiler catches this automatically.
