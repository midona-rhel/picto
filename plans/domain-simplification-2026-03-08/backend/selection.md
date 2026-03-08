# Backend Selection

Current footprint: about 5 files and about 700 lines

## What This Should Own

1. selection query specs
2. selection summary queries
3. selection-scoped bulk operations

## What This Should Not Own

1. frontend selection interaction state
2. grid query semantics duplicated from grid

## Why It Is Too Complicated

1. Selection and grid share scope logic but are still not clearly unified.
2. Selection is smaller than other domains, but drift here breaks many workflows.
3. The domain must stay boring; right now it is still partially coupled to grid internals.

## Simplification Target

1. one shared scope resolver
2. one selection summary service
3. bulk operations routed through domain mutations cleanly

## Concrete Work

1. Reuse the same scope builder as grid.
2. Keep bulk-operation semantics close to selection command handling.
3. Avoid inventing extra selection-only query logic where grid semantics already exist.

## Delete Or Merge

1. Merge duplicated scope helpers with grid.
2. Delete selection code that is really frontend interaction logic.

## Test Target

1. bulk tag workflow on a selection
2. bulk notes or source URL workflow
3. summary query workflow
