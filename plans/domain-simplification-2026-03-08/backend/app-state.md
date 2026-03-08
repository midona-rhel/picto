# Backend App State

## What This Should Own

1. opening and closing a library
2. process-level runtime lifecycle
3. worker startup and shutdown
4. access to the authoritative app state handle

## What This Should Not Own

1. domain business logic
2. UI-facing event shaping
3. query logic
4. transport concerns

## Why It Is Too Complicated

1. `core/src/state.rs` is still acting like a service locator plus lifecycle coordinator.
2. `core/src/runtime_state.rs` and `core/src/events.rs` split runtime concerns in ways that are still too implicit.
3. Library lifecycle, worker lifecycle, and event publishing are too close together.

## Simplification Target

1. one `app runtime` module for startup and teardown
2. one `runtime task registry` module for progress/task state
3. zero domain logic in state boot code

## Concrete Work

1. Move library open/close and worker orchestration behind a small runtime service.
2. Remove direct domain coupling from `state.rs`.
3. Make state acquisition boring: get state, fail if closed, nothing else.
4. Move anything UI-specific out of app-state code.

## Delete Or Merge

1. Merge scattered runtime lifecycle code into one app runtime module.
2. Delete helper indirection that only forwards state access.

## Test Target

1. one orchestration test for library open and close
2. one test for worker startup and cleanup
3. one test for runtime task registry reset on library switch
