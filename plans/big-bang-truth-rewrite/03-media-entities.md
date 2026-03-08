# Media Entities

## Purpose

Define the canonical library item and lifecycle.

## Current Truth

- Code mixes raw file identity, logical entity identity, and collection projection rules.
- Several APIs still expose `file` names even when they operate on logical items.

## Target Truth

- `MediaEntity` is the canonical library item.
- `MediaKind` is one of `image | video | pdf | document | other`.
- `LifecycleState` is `inbox | active | trash`.
- Blob/file storage is implementation detail under the media entity model.

## Rename Map

- logical `FileInfo`/`Entity*` public names -> `MediaEntity*`
- `file_status` public semantics -> `lifecycle_state`
- `get_file`, `delete_file`, `wipe_all_files` -> media-entity equivalents

## Delete List

- Delete DTOs that expose raw file-first naming where logical entity naming is intended.
- Delete duplicate frontend status helpers that reinterpret backend lifecycle semantics.

## DTOs and Commands Involved

- `EntitySlim`
- `EntityAllMetadata`
- `update_file_status`
- import and delete lifecycle commands

## Workflows

- Import blob -> create `MediaEntity` -> initial lifecycle `inbox`.
- Accept item -> `active`.
- Move item to trash -> excluded from `AllActive`, folders, and smart folders unless explicitly viewing trash.

## Acceptance Criteria

- Public docs and code describe library items as media entities.
- Lifecycle semantics are expressed once in backend-owned types.
- No UI feature needs to infer entity rules from raw file storage concerns.
