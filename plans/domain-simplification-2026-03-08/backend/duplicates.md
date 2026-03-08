# Backend Duplicates

Current footprint: about 4 files and about 1.4k lines

## What This Should Own

1. duplicate detection
2. duplicate pair queries
3. duplicate resolution rules

## What This Should Not Own

1. generic file lifecycle behavior
2. renderer notification policy

## Why It Is Too Complicated

1. Duplicate resolution reaches into file lifecycle and tag merging concerns.
2. The domain is not the largest, but it is at risk of being a policy dump.
3. The line between detection and resolution needs to stay explicit.

## Simplification Target

1. matching service
2. decision or merge service
3. query layer for review surfaces

## Concrete Work

1. Split scan and review from merge or delete decisions.
2. Route all side effects through standard file and tag mutation paths.
3. Keep emitted runtime facts minimal and explicit.

## Delete Or Merge

1. Delete duplicate side-effect helpers that bypass core mutation flows.
2. Merge duplicate resolution policies into one decision module.

## Test Target

1. scan then resolve pair workflow
2. auto-merge happy path workflow
3. auto-merge failure event workflow
