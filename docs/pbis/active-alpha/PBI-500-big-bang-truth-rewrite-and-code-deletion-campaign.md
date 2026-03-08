# PBI-500: Big-Bang truth rewrite and code deletion campaign

## Priority
P0

## Problem
The project still carries migration architecture, duplicate ownership, stale naming, and renderer-side domain logic that make a relatively small product feel much larger than it is.

## Goal
Execute a deletion-first rewrite that forces the repo to match `plans/big-bang-truth-rewrite/` and materially reduces line count through boundary collapse, domain realignment, and test simplification.

## Non-Negotiable Rules
1. Rust owns canonical state, query semantics, mutations, receipts, and tasks.
2. TypeScript owns presentation and interaction state only.
3. The runtime contract is `get_runtime_snapshot`, `runtime/mutation_committed`, `runtime/task_upserted`, `runtime/task_removed`, and library lifecycle events.
4. No new compatibility wrappers, renderer-owned domain semantics, or pass-through controllers are allowed during execution.
5. Every checkpoint must typecheck and pass focused tests, even if some app behavior is temporarily incomplete.

## Ordered Execution
1. `PBI-501` canonical naming break
2. `PBI-502` renderer boundary collapse
3. `PBI-503` runtime contract purge
4. `PBI-504` frontend state topology reset
5. `PBI-505` media entity and lifecycle realignment
6. `PBI-506` collections as aggregate projection
7. `PBI-507` tags domain rewrite
8. `PBI-508` folders and smart folders simplification
9. `PBI-509` grid and scope model unification
10. `PBI-510` sidebar and navigation read model
11. `PBI-511` inspector and metadata consolidation
12. `PBI-512` subscriptions and gallery-dl simplification
13. `PBI-513` PTR internalization
14. `PBI-514` app shell and shared-layer deletion
15. `PBI-515` CSS and UI primitive consolidation
16. `PBI-516` test strategy rewrite

## Acceptance Criteria
1. The repo uses `plans/big-bang-truth-rewrite/` as canonical truth.
2. Frontend/backend communication is one typed command surface plus one runtime event surface.
3. Visible PTR product surface is gone.
4. Shared pass-through controller layers are gone.
5. `runtimeSyncStore` is no longer a god-store.
6. Grid, sidebar, tags, folders, collections, subscriptions, and inspector consume backend-owned read models.
7. Test strategy shifts from micro-unit sprawl to workflow proof.

## Test Gates
1. `npx tsc -p tsconfig.json --noEmit`
2. Focused Vitest suites for touched domains
3. One workflow or integration test added or updated when a domain boundary changes
