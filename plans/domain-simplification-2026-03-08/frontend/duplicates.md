# Frontend Duplicates

Current footprint: `src/features/duplicates`, about 3 files and about 793 lines

## What This Should Own

1. duplicate review UI
2. duplicate resolution actions

## What This Should Not Own

1. duplicate policy
2. file lifecycle orchestration beyond calling backend actions

## Why It Is Too Complicated

1. The feature is not huge, which is good.
2. The main risk is letting resolution UX duplicate backend decision rules.

## Simplification Target

1. query and review view model
2. simple review UI

## Concrete Work

1. Keep review pagination local.
2. Let backend own merge or delete semantics.
3. Keep the frontend focused on compare, choose, confirm.

## Delete Or Merge

1. Delete duplicate resolution heuristics from the renderer.

## Test Target

1. fetch pair, resolve pair, refresh workflow
