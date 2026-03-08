# Grid And Query Scopes

## Purpose

Define one query model for all main media browsing.

## Current Truth

- Grid scope logic is powerful but too fragmented.
- Scope, filter, status, collection, folder, and smart-folder rules are spread across multiple layers.

## Target Truth

- One backend grid query service handles:
  - scope kind
  - lifecycle filter
  - tag filters
  - folder filters
  - smart-folder predicate
  - collection scope
  - color filters
  - sort
  - cursor
- General scopes exclude collection members automatically.

## Rename Map

- `GridPageSlimQuery` -> target name may become `MediaQuery`
- any file-first public wording -> media-entity wording

## Delete List

- Delete parallel grid query semantics by domain.
- Delete duplicate frontend orchestration paths for scope resolution and invalidation.

## DTOs and Commands Involved

- `GridPageSlimQuery`
- `GridPageSlimResponse`
- `get_grid_page_slim`
- metadata batch fetch commands used by grid

## Workflows

- Open All Media -> query active scope.
- Open folder -> query folder scope with folder ordering.
- Open collection -> query collection member scope.
- Add filters -> same query model expands; no special-case UI fetch path.

## Acceptance Criteria

- Every main browsing view maps to one grid query structure.
- Collection member exclusion is enforced centrally.
- Cursor paging semantics do not vary by domain.
