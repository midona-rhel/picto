# PBI-505: Media entity and lifecycle realignment

## Status
Implemented

## What shipped
1. The active library bitmap semantics now treat the default library as `active` only.
2. Active metadata/detail command surfaces were renamed from file-centric names to entity-centric names.
3. Collection content metadata stopped acting like independent collection-owned truth.
4. Collection tag edits now fan out to child members, while child-specific tags remain intact unless explicitly changed.
5. Collection rating and source-url writes were removed from the active collection editing surface.
6. Collection mutation receipts now invalidate the scopes that actually change:
   - collection scope
   - normal library scope
   - affected folder scopes on destructive delete
7. Active collection API surfaces no longer expose a fake collection `description` field.

## Final model
1. `MediaEntity` is the logical library item.
2. Lifecycle is only `inbox | active | trash`.
3. Collection membership is not lifecycle.
4. Collections own structure, not independent content metadata.
5. Collection delete is destructive to the aggregate, child media entities, and underlying files.

## Notes
1. Physical storage naming (`file`, `entity_file`, blob/media IO) was intentionally left intact.
2. This PBI did not rename SQLite tables.
