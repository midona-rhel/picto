# Frontend Smart Folders

Current footprint: `src/features/smart-folders`, about 13 files and about 1.8k lines

## What This Should Own

1. smart-folder editing UI
2. predicate builder UI

## What This Should Not Own

1. backend predicate semantics duplicated in the renderer
2. sidebar ownership confusion

## Why It Is Too Complicated

1. Predicate editing is UI-heavy, but the semantic model must stay consistent with the backend.
2. Smart folders leak into sidebar, grid, and tags, so the editing UI can easily take on too much policy.

## Simplification Target

1. one predicate editor model
2. one CRUD surface
3. no renderer-side semantic drift

## Concrete Work

1. Keep the predicate builder strictly schema-driven.
2. Push counting and validation into typed API calls where possible.
3. Make the UI operate on one explicit smart-folder DTO shape.

## Delete Or Merge

1. Delete repeated type coercion and payload casting.
2. Merge small field config files if they do not buy clarity.

## Test Target

1. create smart folder workflow
2. edit predicate then count workflow
3. duplicate and delete workflow
