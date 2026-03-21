# Tags

## Purpose

Describe canonical tags, aliases, implications, and ingest coercion.

## Current Truth

- The core tag storage model is mostly sound.
- Complexity comes from mixing local tags, ingest rules, relation rules, renderer parsing, and dormant PTR-facing logic.

## Target Truth

- Canonical tag storage is `(namespace, value)`.
- `TagAlias` replaces “sibling”.
- `TagImplication` replaces “parent”.
- Merge is destructive canonicalization and is distinct from aliasing.
- Namespace escaping and ingest coercion are backend-owned.

## Rename Map

- `sibling` -> `alias`
- `parent` -> `implication`
- `get_tag_siblings_for_tag` -> `get_tag_aliases`
- `get_tag_parents_for_tag` -> `get_tag_implications`
- `add_tag_parent` -> `add_tag_implication`
- `remove_tag_parent` -> `remove_tag_implication`

## Delete List

- Delete renderer-side ingest namespace policy.
- Delete visible PTR tag source mode.
- Delete duplicate picker and relation UIs that each fetch or group tags differently.

## DTOs and Commands Involved

- `TagSearchResult`
- `TagRecord`
- `TagRelation`
- tag search, rename, merge, alias, implication, batch add/remove commands

## Workflows

- Add local tag -> parse on backend -> store canonical tag -> rebuild effective tag projections.
- Add alias -> visible tag changes, canonical storage does not.
- Add implication -> effective tag set expands for matching entities.
- Import external metadata -> coerce unknown namespaces once on ingest.

## Acceptance Criteria

- User-facing UI says `Alias` and `Implication`.
- Frontend no longer reparses stored tag semantics.
- Tag workflows are explainable without mentioning PTR or UI maintenance actions.
