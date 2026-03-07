# PBI-401: Frontend root and domain topology realignment

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `src/App.tsx` imports from both feature-first surfaces and legacy direct component/service paths.
2. The codebase still has parallel ownership trees such as `src/components/sidebar/` and `src/features/sidebar/`.
3. High-value screens like flows, tags, collections, duplicates, and parts of the sidebar still live primarily in legacy `src/components/` paths.
4. `src/` root currently contains a flat row of app entry files and shell files:
   - `App.tsx`
   - `detail.tsx`
   - `library-manager.tsx`
   - `main.tsx`
   - `settings.tsx`
   - `subscriptions.tsx`
5. Shared vs domain-owned code is not visually obvious from the directory layout, so it is hard to answer basic maintenance questions:
   - what belongs to a feature/domain
   - what is a shared primitive
   - what should be deleted
   - what is only legacy compatibility residue

## Problem
The frontend does not have a clear topological structure. The problem is not just "some files are still in `src/components`"; it is that the project does not visibly tell you:

1. which folders are application shell / entrypoints
2. which folders are domain-owned
3. which folders are shared cross-domain building blocks
4. which folders are temporary migration residue

That makes the codebase hard to reason about and encourages further drift.

## Scope
- Entire frontend layout under `src/`
- Root entrypoint/layout files
- Domain folders in `src/features/`
- Shared code folders (`components`, `hooks`, `lib`, `types`, `styles`, `desktop`)
- Legacy residues that should be moved, merged, or deleted

## Implementation
1. Define the target frontend structure explicitly.

### Target structure

```text
src/
  app/                  # app shell, composition root, bootstrap wiring
  entrypoints/          # main.tsx/detail.tsx/settings.tsx/subscriptions.tsx/library-manager.tsx
  features/             # domain-owned UI + domain hooks/controllers per feature
    grid/
    sidebar/
    folders/
    tags/
    subscriptions/
    settings/
    collections/
    duplicates/
    viewer/
    layout/
    app/
  shared/               # cross-domain primitives only
    components/
    hooks/
    lib/
    styles/
    types/
    services/
  state/                # app-global stores that are not owned by one feature
  platform/             # Electron/desktop adapters and transport wrappers
  test/
```

2. Define hard placement rules:
   - `src/` root may contain only entrypoint files, `vite-env.d.ts`, and top-level style/module files required by the bundler.
   - Domain UI or domain logic must not live directly in `src/`.
   - `src/features/*` owns domain-specific components, hooks, controllers, and local helpers.
   - `src/shared/*` contains only genuinely cross-domain code.
   - `src/platform/*` contains Electron/desktop bridge code; it should not be mixed into generic shared utilities.
   - `src/state/*` contains global stores that are app-level rather than domain-level.

3. Classify every current top-level frontend folder/file into one of:
   - keep as-is
   - move into feature
   - move into shared
   - move into platform
   - merge/delete as legacy residue

4. Perform the first cleanup pass on the most obvious structural offenders:
   - move `App.tsx` shell concerns under `src/app/`
   - move root screen entry shells under `src/entrypoints/`
   - stop using `src/components/*` as a second domain tree
   - identify which files under `src/components/` are actually feature-owned and move them

5. Produce a keep/delete table for the old top-level buckets:
   - `src/components/`
   - `src/hooks/`
   - `src/services/`
   - `src/desktop/`
   - `src/controllers/`
   - `src/stores/`

6. Add migration rules to prevent relapse:
   - new domain code goes in `src/features/*`
   - new cross-domain primitives go in `src/shared/*`
   - new Electron bindings go in `src/platform/*`

## Acceptance Criteria
1. A target `src/` topology is explicitly documented and used for migration.
2. The root of `src/` is reduced to entrypoints and unavoidable build-level files only.
3. There is a clear distinction between:
   - app shell
   - feature/domain code
   - shared code
   - platform/Electron code
   - app-global state
4. `src/components/*` is no longer a catch-all domain tree.
5. A reviewer can tell whether a file should be moved, kept, or deleted based on directory rules rather than project folklore.

## Test Cases
1. Inspect `src/` root — only entrypoints/build files remain.
2. Inspect a moved domain such as subscriptions or tags — its code lives under a single feature-owned subtree.
3. Inspect shared primitives — they are cross-domain and not secretly feature-owned.
4. Run `npx tsc -p tsconfig.json --noEmit` after each migration slice.

## Risk
High. This is a large-scale structural cleanup. It must be done in slices, but the target layout itself needs to be explicit up front or the codebase will keep drifting.

## Execution Clarification (2026-03-07)

`PBI-401` defines the target topology, but it is not sufficient by itself.
Execution should be staged through:

1. `PBI-405` frontend topology policy and CI guardrails
2. `PBI-406` frontend legacy register and classification pass
3. `PBI-407` frontend legacy deletion campaign and compatibility shim purge
4. `PBI-404` split oversized frontend orchestration modules

The structure work is only complete when legacy paths are blocked from regrowth
and obsolete paths are actually deleted.
