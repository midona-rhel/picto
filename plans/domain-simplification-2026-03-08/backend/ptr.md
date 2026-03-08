# Backend PTR

Current footprint: about 11 files and about 7.4k lines

## What This Should Own

1. PTR bootstrap
2. PTR sync
3. PTR cache and overlay behavior
4. PTR query surface

## What This Should Not Own

1. global runtime state
2. generic tag domain behavior
3. renderer progress formatting

## Why It Is Too Complicated

1. PTR is currently its own mini-application inside the backend.
2. Bootstrap, sync, cache, overlay, query logic, and runtime reporting are still too close together.
3. The domain is large enough that naming alone is no longer enough; it needs real seams.

## Simplification Target

1. bootstrap service
2. sync service
3. cache or overlay service
4. query service
5. runtime task adapter

## Concrete Work

1. Separate import/bootstrap code from live sync code.
2. Move runtime progress shaping behind the task model.
3. Keep PTR-specific persistence isolated from general storage.

## Delete Or Merge

1. Delete duplicated runtime state handling.
2. Merge tiny PTR helper modules only when they are not real services.

## Test Target

1. bootstrap workflow
2. sync workflow
3. tag query workflow against PTR data
