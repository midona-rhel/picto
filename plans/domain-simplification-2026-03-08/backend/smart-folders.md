# Backend Smart Folders

Current footprint: about 3 files and about 1k lines

## What This Should Own

1. smart-folder CRUD
2. predicate validation
3. smart-folder query execution

## What This Should Not Own

1. generic tag parsing
2. sidebar refresh policy
3. renderer-facing normalization quirks

## Why It Is Too Complicated

1. Smart-folder semantics depend on tags, grid scope, and sidebar behavior at once.
2. Predicate behavior is not isolated enough from the rest of the query stack.
3. The domain is smaller than subscriptions or PTR, but it still suffers from cross-domain leakage.

## Simplification Target

1. one predicate model
2. one executor for smart-folder queries
3. one CRUD surface

## Concrete Work

1. Keep predicate parsing and validation local to this domain.
2. Use the shared scope/query core rather than ad hoc composition.
3. Stop returning renderer-shaped oddities that require normalization later.

## Delete Or Merge

1. Merge predicate helpers into one focused module.
2. Delete frontend normalization hacks by fixing the backend shape.

## Test Target

1. create and update predicate workflow
2. count and query workflow
3. rename and delete workflow
