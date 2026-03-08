# Frontend Layout

Current footprint: `src/features/layout`, about 8 files and about 892 lines

## What This Should Own

1. view routing and layout composition
2. shell-level panels and window controls

## What This Should Not Own

1. domain orchestration
2. feature-specific state derivation

## Why It Is Too Complicated

1. Layout should be a thin composition feature.
2. It becomes a problem when it starts carrying feature state just because it sits in the middle.

## Simplification Target

1. route and composition only
2. no hidden business rules

## Concrete Work

1. Keep router-level composition clean.
2. Use explicit providers and view models.
3. Avoid hiding domain policy in layout providers.

## Delete Or Merge

1. Delete provider layers that only forward props.
2. Merge tiny layout helpers if they fragment one obvious responsibility.

## Test Target

1. view-router integration tests
