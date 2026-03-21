# Collections

## Purpose

Define grouped media entities as first-class aggregate entities.

## Current Truth

- Collections already behave like grouped entities, but naming and query rules are spread between grid, selection, inspector, and metadata code.
- Hidden-member behavior is implicit and easy to miss.

## Target Truth

- `Collection` is a `MediaEntity`.
- `CollectionMember` rows associate members to a collection.
- Members are hidden from normal scopes by query and projection rules.
- Collection summary is backend-derived from member entities.

## Rename Map

- `collection_item_count` may remain as summary field, but all rule docs describe membership as visibility projection.
- “collection status” wording -> deleted

## Delete List

- Delete any attempt to model collection membership as lifecycle.
- Delete duplicate collection-summary mapping in frontend hooks.

## DTOs and Commands Involved

- `CollectionInfo`
- `CollectionSummary`
- `create_collection`
- `add_collection_members`
- `remove_collection_members`
- `reorder_collection_members`

## Workflows

- Select items -> create collection -> new collection entity created.
- Add members -> member entities vanish from general grid scopes.
- Open collection scope -> show ordered member grid and summary metadata.

## Acceptance Criteria

- Collections are explained without inventing a fourth lifecycle state.
- Hidden-member behavior is enforced in one backend query model.
- Inspector and grid consume collection DTOs directly.
