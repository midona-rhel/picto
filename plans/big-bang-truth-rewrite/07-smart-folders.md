# Smart Folders

## Purpose

Define computed folder scopes backed by predicates and bitmaps.

## Current Truth

- Smart folders are conceptually right, but predicate shape, counting, UI editing, and query use are split too widely.

## Target Truth

- `SmartFolder` is a saved predicate scope.
- Membership is computed on read or compile, never stored as manual membership.
- Predicates are backend-owned and compiled against roaring bitmaps and SQL projections.

## Rename Map

- keep `SmartFolder`
- remove wording that suggests smart folders are normal folders with live membership rows

## Delete List

- Delete frontend predicate semantics duplication.
- Delete any ad hoc smart-folder refresh logic outside runtime receipt handling.

## DTOs and Commands Involved

- `SmartFolder`
- `SmartFolderPredicate`
- `create_smart_folder`
- `update_smart_folder`
- `count_smart_folder`
- `query_smart_folder`

## Workflows

- Create smart folder -> validate predicate -> compile count preview.
- Save smart folder -> bitmap projection updates.
- Mutate tags/folders/lifecycle -> affected smart folder scopes become stale and refresh via runtime receipts.

## Acceptance Criteria

- Smart folders are documented as computed scopes only.
- Predicate behavior is identical between count, query, and sidebar count surfaces.
- Frontend editor is schema-driven, not semantics-driven.
