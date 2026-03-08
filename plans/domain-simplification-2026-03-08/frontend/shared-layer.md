# Frontend Shared Layer

Current footprint: `src/shared`, about 237 files and about 10.1k lines

## What This Should Own

1. truly shared UI primitives
2. generic hooks with multiple real consumers
3. generic utilities
4. transport-neutral shared types

## What This Should Not Own

1. unresolved feature ownership
2. fake controllers
3. feature policies hidden as reusable code

## Why It Is Too Complicated

1. `shared` is too large because it is absorbing uncertainty.
2. `shared/controllers` is especially suspect because many files are thin wrappers.
3. Once a codebase starts hiding feature behavior under `shared`, nobody knows where to make changes.

## Simplification Target

1. shared primitives only
2. no feature orchestration in `shared`
3. most domain-facing controller files gone

## Concrete Work

1. Audit `shared/controllers` and keep only modules with real behavior.
2. Move feature-specific code back into features.
3. Keep only cross-feature UI primitives and reusable hooks.

## Delete Or Merge

1. Delete pass-through controllers.
2. Delete duplicated context-menu or action registries if they only mirror feature policy.

## Test Target

1. shared primitive tests only where behavior is reusable and non-trivial
2. stop spending tests on registry shape if workflow tests cover the same outcome
