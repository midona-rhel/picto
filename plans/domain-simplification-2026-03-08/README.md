# Domain Simplification Plan

Date: 2026-03-08
Scope: current `core/`, `src/`, `electron/`, and test layout
Audience: whoever actually has to simplify this codebase rather than write another audit about it

## Actual Goal

This app is an image album manager with tags, folders, viewer, duplicates, subscriptions, and some PTR/sync machinery.

The real goal is not "clean architecture". The real goal is:

1. one authoritative backend state model
2. one small typed bridge to the renderer
3. one coherent runtime invalidation/task model
4. thin feature code in the renderer
5. fewer files, fewer layers, fewer duplicate orchestration paths

## Hard Truths

1. A 70 percent total line-count reduction is not realistic unless product scope is also cut. A 30 to 50 percent reduction in orchestration, wrappers, shims, stale docs, and test clutter is realistic.
2. `shared` is not a real domain. It is where unresolved ownership goes to hide.
3. A controller that only renames an API call is not architecture. It is drag.
4. Compatibility layers are acceptable only when they come with a deletion date.
5. The project currently has more migration structure than product structure.

## Current High-Cost Areas

1. `src/features/grid` is about 82 files and about 18.3k lines.
2. `src/shared` is about 237 files and about 10.1k lines.
3. `core/src/ptr` is about 11 files and about 7.4k lines.
4. `core/src/subscriptions` is about 7 files and about 6.6k lines.
5. `core/src/sqlite` is about 7 files and about 6.2k lines.
6. `core/src/dispatch` is about 15 files and about 5.2k lines.

Those are not just "large". They are where ownership drift is being financed.

## Target Shape

1. Rust owns:
   - library state
   - persistence
   - queries
   - mutations
   - background jobs
   - mutation receipts
   - task progress
2. TypeScript owns:
   - selection state
   - open views and panels
   - drag, hover, scroll, zoom, transition state
   - local caches that are purely for rendering
3. Bridge owns only:
   - typed commands
   - runtime snapshot
   - mutation receipts
   - task events
   - library lifecycle events

## Sequence

1. fix contract and guardrails first
2. collapse thin frontend wrappers
3. simplify runtime sync
4. slim app shell and shared layer
5. simplify domains one by one
6. replace test sprawl with workflow coverage

## Folder Layout

1. `backend/` contains per-domain backend plans
2. `frontend/` contains per-domain frontend plans
3. `tests/` contains the testing strategy and migration rules

## Truth Docs

These are the docs that explain how a domain actually works today and what its rewrite boundary should be.

1. `tags-system-truth.md`

## Domain Index

Backend:

1. `app-state`
2. `dispatch`
3. `sqlite`
4. `events-runtime-contract`
5. `grid`
6. `folders`
7. `smart-folders`
8. `tags`
9. `subscriptions`
10. `duplicates`
11. `selection`
12. `sidebar`
13. `settings`
14. `ptr`
15. `import`
16. `metadata`
17. `lifecycle`
18. `media-processing`
19. `scope`

Frontend:

1. `app-shell`
2. `platform-bridge`
3. `runtime-sync`
4. `state-stores`
5. `shared-layer`
6. `grid`
7. `sidebar`
8. `subscriptions`
9. `tags`
10. `viewer`
11. `smart-folders`
12. `collections`
13. `duplicates`
14. `settings`
15. `inspector`
16. `layout`
17. `folders`

Testing:

1. `tests/strategy.md`

## Use This Plan Correctly

1. Do not rewrite the whole app in one branch.
2. Every phase must delete more code than it adds.
3. If a layer cannot justify itself in one sentence, remove it.
4. Prefer one obvious module over three polite abstractions.
