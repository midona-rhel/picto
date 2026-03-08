# Inspector And Metadata

## Purpose

Define selection-driven metadata viewing and editing.

## Current Truth

- Inspector behavior is useful but split across fetch hooks, mutation hooks, collection mapping, and tag parsing helpers.
- Some metadata concepts, especially site-derived time fields, are not centralized.

## Target Truth

- Inspector is a presentation surface over backend metadata DTOs.
- Backend returns resolved metadata once.
- Inspector edits tags, notes, source URLs, rating, folders, and collection properties through commands only.
- Site-time metadata is centralized under metadata domain when implemented.

## Rename Map

- file metadata public wording -> media-entity metadata
- `parent_tags` UI wording -> implied tags if exposed to users

## Delete List

- Delete frontend reparsing of resolved tags.
- Delete duplicate collection-tag mapping in inspector hooks.
- Delete frontend metadata normalization that the backend can return directly.

## DTOs and Commands Involved

- `EntityAllMetadata`
- `ResolvedTagInfo`
- selection summary DTOs
- notes, rating, source URL, folder membership commands

## Workflows

- Select one entity -> inspector shows resolved metadata.
- Select many -> inspector shows shared metadata and batch mutations.
- Select collection -> inspector shows collection summary and collection-level metadata.

## Acceptance Criteria

- Inspector is explainable as one read-model consumer plus mutation commands.
- No frontend hook has to rebuild backend tag semantics.
- Site-time metadata has one documented home even if implementation is deferred.
