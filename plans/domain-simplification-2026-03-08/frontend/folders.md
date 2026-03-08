# Frontend Folders

Current footprint: `src/features/folders` is effectively empty

## What This Means

1. This is not currently a real frontend domain.
2. Folder UI behavior is actually spread across sidebar, grid, inspector, and shared controllers.

## Why This Is A Problem

1. The codebase has a domain name without a domain boundary.
2. That is worse than either having a real feature or admitting folders are owned by sidebar plus grid interactions.

## Simplification Target

1. either create a real folder feature boundary
2. or delete `src/features/folders` and stop pretending

## Concrete Work

1. Decide where folder UI actually belongs.
2. Most likely split is:
   - sidebar owns folder tree interactions
   - grid owns folder membership interactions
   - inspector owns per-file folder membership editing
3. If that split stands, remove `features/folders` entirely.

## Delete Or Merge

1. Delete empty or placeholder feature structure.

## Test Target

1. folder workflows should live under sidebar or grid integration tests, not a fake feature
