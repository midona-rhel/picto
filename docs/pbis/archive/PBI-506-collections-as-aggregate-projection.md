# PBI-506: Collections as aggregate projection

## Status
Implemented

## What shipped
1. Collections are explicit `media_entity` aggregates with membership.
2. Collection summaries are backend-owned derived read models.
3. General scopes exclude grouped members.
4. Collection scopes are handled by backend query/read-model paths rather than renderer-only exceptions.

## Implementation notes
1. Collection reads are owned by `./core/src/folders/collections_db.rs`.
2. Collection grid scopes are handled in `./core/src/grid/query/collection.rs`.
3. Non-collection scope exclusion for grouped members is encoded in:
   - `./core/src/sqlite/files.rs`
   - `./core/src/selection/summary.rs`
   - `./core/src/tags/compiler.rs`
