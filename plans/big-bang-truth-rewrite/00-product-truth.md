# Product Truth

## Purpose

Describe the app as it should exist after the rewrite, not as the current code accidentally describes it.

## Current Truth

- The app is an image-first media library, but code and docs still mix `file`, `entity`, `collection`, PTR, and view-specific language.
- Backend authority exists, but frontend still duplicates semantics, invalidation, and parsing in several domains.
- PTR is visible in product UI even though it is dormant and not part of the main user story.

## Target Truth

- The app is a `MediaEntity` library.
- A `MediaEntity` may be image, video, PDF, document, or other file-backed media.
- `LifecycleState` is only `inbox | active | trash`.
- `Collection` is a special `MediaEntity` with `CollectionMember` rows; members are hidden by projection rules, not by status.
- Tags, folders, smart folders, subscriptions, grid, sidebar, inspector, and settings are the live product domains.
- Runtime is `RuntimeSnapshot` plus typed deltas only.

## Rename Map

- `file` -> `media_entity` where the logical item is meant, not the raw blob.
- `flow` -> `subscription_group`.
- `sibling` -> `alias`.
- `parent` -> `implication`.
- visible `PTR` -> removed from product terminology.

## Delete List

- Delete visible PTR product UI.
- Delete legacy compatibility event names after runtime cutover.
- Delete duplicate frontend controller or portal layers that only rename backend calls.
- Delete docs that describe migration topology instead of product truth.

## DTOs and Commands Involved

- `EntitySlim`, `EntityAllMetadata`, `CollectionSummary`, `SelectionSummary`
- `get_runtime_snapshot`
- `runtime/mutation_committed`
- `runtime/task_upserted`
- `runtime/task_removed`

## Workflows

- Import media -> enters `inbox` -> user accepts to `active` or moves to `trash`.
- Create collection -> assign members -> members disappear from general scopes -> collection summary shows aggregate view.
- Add tag alias or implication -> grid, inspector, and search all resolve the same way.
- Run subscriptions on schedule -> gallery-dl fetches -> dedupe -> import pipeline -> runtime task updates.

## Acceptance Criteria

- A contributor can explain the app without mentioning shims, controller facades, or PTR product UI.
- Every active feature maps cleanly to one of the target domains above.
- No product doc requires a second doc to explain its core nouns.
