# PBI-612: Add an in-library image-similarity filter

## Problem

reference application's Image filter accepts an existing library item or external image and returns visually similar
items inside the current library query. Picto currently offers only external reverse-image-search
actions and duplicate detection. Neither is a truthful substitute: external search leaves the library,
while duplicate detection is pair-review infrastructure rather than a composable grid predicate.

## Contract

- Add a canonical image-similarity filter input that references either a library entity or a
  temporary query image without storing base64 data in URL/UI state.
- Resolve similarity in the backend and compose it with the current scope, lifecycle, text, tags,
  folders, rating, type, date, size, resolution, Notes, and URL predicates.
- Define and persist the embedding or feature projection needed for every supported image format,
  including invalidation when media is replaced.
- Return stable similarity scores and typed cursors so paging, counts, selection, exports, and writes
  operate on the same ordered result set.
- Reuse the existing filter row and shared `ContextMenu`; do not route the filter through an external
  search engine or the duplicate-review scan.

## Verification

- Fixed visual fixtures prove stable ordering for exact, near, and unrelated images.
- An external query image and an existing library entity produce the same ordering when their pixels
  are identical.
- Scope and every canonical predicate compose without client-side filtering or loaded-page bias.
- Paging, counts, query-wide selection, export, and mutation targets resolve the same matching IDs.
- Unsupported media and failed projections produce explicit, recoverable states rather than empty
  results that look authoritative.

Delete this PBI when the acceptance checks pass. Git history is the archive.
