# PBI-525: Structured error types at dispatch boundary

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The severity assessment is based on code inspection of error propagation paths. The actual user impact depends on how frequently errors occur in practice and whether the current string-based errors are causing real confusion. A survey of recent error logs should validate the premise.

## Priority
P1

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: Every error in the Rust core is coerced to `String` via `format!()` or `.map_err(|e| format!(...))`. The dispatch function signature is `Result<String, String>`. The frontend receives opaque error strings with no structured category information. Verified by reading `core/src/dispatch/mod.rs` (line 52), the `call!` macro (line 13), and tracing through error propagation in `core/src/sqlite/diagnostics.rs`, `core/src/state.rs`, and domain handlers.

## Problem
All errors in the Rust core are collapsed to `String` at the earliest possible point. The dispatch boundary (`dispatch() -> Result<String, String>`) provides no structured error information to the frontend.

Consequences:
- The frontend cannot distinguish between "file not found" (show a toast), "database locked" (retry), and "library corrupt" (prompt recovery). Every error is an opaque string.
- No error codes means the frontend can only pattern-match on error message text, which is fragile and breaks across Rust code changes.
- Stack traces and error context are lost — when a deeply nested SQLite query fails, the error string says *what* but not *where*.
- Structured error logging and telemetry (aggregate error rates by category) is impossible.

## Scope
- `core/src/dispatch/mod.rs` — dispatch return type and error formatting
- `core/src/dispatch/common.rs` — `to_json()` and error conversion helpers
- `native/picto-node/src/lib.rs` — N-API error marshaling
- `electron/nativeClient.mjs` — error parsing at the bridge
- `src/platform/**` — frontend error handling

## Implementation
1. **Define `DispatchError` enum** in `core/src/dispatch/common.rs`:
   ```rust
   #[derive(Debug, Serialize)]
   pub struct DispatchError {
       pub code: ErrorCode,
       pub message: String,
   }

   #[derive(Debug, Serialize)]
   pub enum ErrorCode {
       NotFound,       // Entity/file not found
       Conflict,       // Duplicate hash, constraint violation
       Validation,     // Invalid input args
       NoLibrary,      // No library is open
       Internal,       // Unexpected / catch-all
   }
   ```
2. **Update dispatch signature** to `Result<String, DispatchError>`. Serialize the error as JSON at the N-API boundary so the frontend receives `{ code: "NotFound", message: "..." }`.
3. **Categorize errors at the source**: Replace `format!("...")` error strings with `DispatchError::not_found(msg)`, `DispatchError::validation(msg)`, etc. Start with the most common handlers (grid, tags, import) and leave the rest as `Internal` — categorization can be incremental.
4. **Frontend error parsing**: Update `api.ts` to parse the structured error. Expose `error.code` for programmatic handling and `error.message` for display.
5. **Error toast foundation**: Wire the structured error into a notification system (see PBI-529 for the full toast system).

## Acceptance Criteria
1. `dispatch()` returns `Result<String, DispatchError>` with a `code` field.
2. Frontend receives `{ code, message }` for all errors, not opaque strings.
3. At least the grid, tag, and import handlers use specific error codes (not all `Internal`).
4. `cargo test` passes; `npx tsc --noEmit` passes.
5. Existing frontend error handling does not regress (errors still caught and logged).

## Test Cases
1. Import a file that already exists → error code is `Conflict`, message mentions duplicate hash.
2. Query a grid page with invalid scope → error code is `Validation`.
3. Call `get_entity` with a nonexistent hash → error code is `NotFound`.
4. Call any command with no library open → error code is `NoLibrary`.
5. Force an internal error (e.g., corrupt query) → error code is `Internal`.

## Risk
Medium. Changing the dispatch return type touches the N-API boundary, which is the critical bridge between Rust and Electron. The migration must be incremental — start by wrapping the existing `String` errors in `DispatchError { code: Internal, message }` so all existing behavior is preserved, then categorize errors one handler at a time.
