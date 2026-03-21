# PBI-534: Tracing subscriber initialization placement

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: In `core/src/state.rs` line 49-54, `tracing_subscriber::fmt().try_init()` is called at the top of `open_library()`. This runs on every library switch, not just the first app startup. The `try_init()` silently fails after the first successful initialization (returns `Err` which is discarded via `let _ =`), so it's harmless but misplaced.

## Problem
`tracing_subscriber::try_init()` is called inside `open_library()`, which runs every time the user switches libraries. The subscriber can only be initialized once per process — subsequent calls silently fail. This is:

1. **Misleading**: A reader might think the tracing configuration is per-library
2. **Wasteful**: The `EnvFilter` parse and `try_init()` attempt run unnecessarily on every library switch
3. **Misplaced**: Initialization belongs in the addon load or process bootstrap, not in a repeated operation

## Scope
- `core/src/state.rs` — `open_library()` function
- `native/picto-node/src/lib.rs` — addon initialization (better location)

## Implementation
1. **Move tracing init** to a dedicated `pub fn init_tracing()` function in `core/src/lib.rs` or `core/src/state.rs`.
2. **Call from N-API addon init**: In `native/picto-node/src/lib.rs`, call `picto_core::init_tracing()` during module registration (runs once per process).
3. **Remove from `open_library()`**: Delete the `tracing_subscriber` block from `open_library()`.

## Acceptance Criteria
1. Tracing is initialized once at addon load, not on every library switch.
2. `RUST_LOG` environment variable still controls log levels.
3. No change in log output behavior.

## Test Cases
1. Open library → switch to another library → verify tracing init runs only once (no duplicate subscriber warning).
2. Set `RUST_LOG=picto=debug` → verify debug logs appear.
3. No `RUST_LOG` set → verify default `picto=info` filter applies.

## Risk
Low. This is a code placement fix with no behavioral change. The only risk is if the N-API addon init runs before the environment variable is set — but `EnvFilter::try_from_default_env()` reads the env var at call time, and env vars are set before the process starts.
