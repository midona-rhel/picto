# PBI-604: Tag manager activation

## Priority
P1

## Problem

The canonical backend already supports paginated tag reads, namespace summaries, rename,
merge, delete, aliases, implications, and site masks. The rebuilt frontend exposes a Tag
Manager sidebar entry but routes it to the generic unavailable placeholder. The manager still
needs a dedicated rebuilt surface backed by the canonical tag API.

## Implementation

1. Add a dedicated rebuilt `TagManager` surface under `src/features/tags/`.
2. Use `src/platform/tagApi.ts` directly through one small feature controller or hook.
3. Support namespace filtering, search, cursor pagination, and empty/loading/error states.
4. Support rename, merge, delete, alias, implication, and site-mask editing.
5. Route `system:tag_manager` as a non-grid manager entry.
6. Add only the layout needed by this surface; do not build a speculative shared manager
   framework beyond this surface.
7. Refresh through normal `runtime/state_changed` tag facts after mutations.

## Acceptance criteria

- Clicking Tag Manager opens the rebuilt manager instead of the unavailable placeholder.
- Tags with zero entities are visible.
- Search and pagination do not duplicate or skip tags.
- Rename, merge, delete, alias, implication, and site-mask edits persist and settle visibly.
- Deleting or merging a tag refreshes affected grid/inspector tag state.
- Focused backend mutation tests and frontend manager interaction tests pass.
- The manager imports only rebuilt `src/**` modules.
