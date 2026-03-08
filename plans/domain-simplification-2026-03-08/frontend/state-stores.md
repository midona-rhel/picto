# Frontend State Stores

Current footprint: `src/state`, about 9 files and about 2.1k lines

## What This Should Own

1. UI state that is not worth pushing into Rust
2. coarse application navigation and settings state
3. cached backend results only when needed for rendering

## What This Should Not Own

1. authoritative domain state that already belongs to the backend
2. duplicate orchestration logic spread across many stores

## Why It Is Too Complicated

1. Stores are reasonably coherent, but they still risk becoming shadow models of backend state.
2. The distinction between "UI state" and "mirrored backend state" is not hard enough yet.

## Simplification Target

1. navigation store
2. settings store
3. render-cache stores
4. minimal domain aggregates derived from backend refreshes

## Concrete Work

1. Audit each store and label it `ui`, `cache`, or `sync`.
2. Kill any store state that can be fetched or derived cheaply from Rust.
3. Keep cross-store dependencies explicit.

## Delete Or Merge

1. Merge stores that exist only because of file-organization churn.
2. Delete mirrored backend state that is never independently authoritative.

## Test Target

1. store behavior tests for real state transitions only
2. no tests for trivial setters
