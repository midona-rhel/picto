# Backend Grid

Current footprint: about 4 files and about 925 lines

## What This Should Own

1. paginated grid queries
2. filter and scope resolution
3. sort and cursor behavior

## What This Should Not Own

1. frontend view preferences
2. selection mutation rules
3. renderer transition logic

## Why It Is Too Complicated

1. Grid logic is split across grid, selection, scope, and bitmap/publish layers.
2. Query semantics are correct more often than they are obvious.
3. Grid and selection still depend on parallel scope logic that can drift.

## Simplification Target

1. one grid query service
2. one shared scope resolver used by grid and selection
3. explicit contracts for filters, sorting, and cursor semantics

## Concrete Work

1. Extract one canonical scope builder.
2. Keep pagination and filtering inside the grid domain.
3. Stop leaking query semantics across unrelated helper modules.

## Delete Or Merge

1. Merge duplicated scope logic with selection.
2. Delete helper layers that only translate between identical query shapes.

## Test Target

1. one integration test for default grid query
2. one for folder and smart-folder scope
3. one for tag and status filters combined
