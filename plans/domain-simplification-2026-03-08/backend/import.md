# Backend Import

Current footprint: about 4 files and about 522 lines

## What This Should Own

1. turning external files into internal entities
2. import transactions
3. import result reporting

## What This Should Not Own

1. metadata extraction pipeline internals
2. subscription orchestration
3. duplicate policy

## Why It Is Too Complicated

1. Import is conceptually simple but touches metadata, lifecycle, subscriptions, and duplicates.
2. The risk is not size; the risk is being used as a dumping ground for unrelated logic.

## Simplification Target

1. import request handling
2. import transaction pipeline
3. handoff to metadata and lifecycle services

## Concrete Work

1. Keep import entrypoints small.
2. Push non-import policies out to owning domains.
3. Make subscription imports reuse the same pipeline rather than shadowing it.

## Delete Or Merge

1. Delete subscription-specific import forks when the standard pipeline can be reused.
2. Merge tiny import helpers if they are just transaction plumbing.

## Test Target

1. manual import workflow
2. import with tags and source URLs workflow
3. import conflict handling workflow
