# Frontend Collections

Current footprint: `src/features/collections`, about 3 files and about 711 lines

## What This Should Own

1. collections UI
2. collection summary and membership actions

## What This Should Not Own

1. generic folder logic
2. generic file mutation logic

## Why It Is Too Complicated

1. Collections are small enough that they should stay simple.
2. The risk is treating collections as "special folders plus exceptions" everywhere.

## Simplification Target

1. one collection view model
2. backend-owned collection semantics

## Concrete Work

1. Keep collection-specific commands together.
2. Make it explicit where collection behavior diverges from folders.
3. Avoid smearing collection rules through grid, sidebar, and folder code.

## Delete Or Merge

1. Delete collection branches in unrelated features when collection view models can own them.

## Test Target

1. create collection, add members, remove members workflow
2. update summary metadata workflow
