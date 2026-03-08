# Frontend Runtime Sync

Current footprint: `src/runtime` plus `src/state/runtimeSyncStore.ts`, about 6 files and about 589 lines plus a 595-line store

## What This Should Own

1. runtime snapshot hydration
2. mutation receipt ingestion
3. task projection into renderer-facing state
4. stale-resource invalidation

## What This Should Not Own

1. feature-specific data loading policy beyond invalidation
2. duplicate domain state that can be queried from Rust

## Why It Is Too Complicated

1. `runtimeSyncStore` currently does too much in one file.
2. It owns subscriptions, flows, PTR, stale resources, timers, and watchdog polling.
3. This is dangerously close to building a second backend in TypeScript.

## Simplification Target

1. one runtime subscription layer
2. one task projection layer
3. one invalidation layer

## Concrete Work

1. Split internals of `runtimeSyncStore` by concern.
2. Keep one public store API, but stop piling every runtime responsibility into one module.
3. Remove legacy runtime fallbacks after task events are authoritative.

## Delete Or Merge

1. Delete duplicate progress/event code once runtime tasks fully cover the same workflows.
2. Merge tiny refresh helpers if they exist only to bounce state around.

## Test Target

1. one mutation receipt to stale-resource workflow test
2. one task projection workflow for subscription or flow progress
3. one library-switch reset workflow
