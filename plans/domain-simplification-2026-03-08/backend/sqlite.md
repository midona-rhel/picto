# Backend SQLite

## What This Should Own

1. schema and migrations
2. low-level persistence
3. read-model publishing
4. transaction boundaries

## What This Should Not Own

1. domain orchestration
2. event policy
3. business-level naming rules

## Why It Is Too Complicated

1. `core/src/sqlite` is about 7 files and about 6.2k lines.
2. Schema, migration history, write persistence, and derived publish logic are still too entangled.
3. SQLite modules are functioning as a second domain layer rather than storage plumbing.

## Simplification Target

1. schema and migration pack split from operational storage code
2. write paths separated from publish/projection code
3. domains call repositories, not random SQLite helpers

## Concrete Work

1. Split schema definition from migration history.
2. Split write repositories from projection/publish modules.
3. Make each domain own the decision to write; SQLite only performs it.
4. Reduce cross-domain SQL utility leakage.

## Delete Or Merge

1. Delete generic storage helpers that hide which tables they actually touch.
2. Merge tiny table-specific modules only when they are purely mechanical.

## Test Target

1. migration boot test
2. one integration workflow per major repository
3. projection refresh tests at repository boundaries, not UI-style micro-tests
