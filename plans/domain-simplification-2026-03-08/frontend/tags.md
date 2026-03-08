# Frontend Tags

Read `../tags-system-truth.md` first. This file is only the frontend slice.

Current footprint: `src/features/tags`, about 11 files and about 2.9k lines

## What This Should Own

1. tag manager UI
2. tag selection UI
3. relation editing UI

## What This Should Not Own

1. backend normalization rules
2. repeated fetch orchestration in components

## Why It Is Too Complicated

1. `TagManager.tsx` is still too large and too stateful.
2. Search, pagination, rename, merge, relations, selection, and notifications still live too close together.
3. The feature still compensates for backend shapes more than it should.
4. The frontend has at least three overlapping tag selection surfaces:
   - `TagSelectPanel`
   - `TagPickerPortal`
   - `TagPickerMenu`
5. Tag parsing, namespace extraction, display formatting, and grouping rules are repeated across manager, inspector, shared helpers, and picker surfaces.
6. The UI still performs backend maintenance behavior on load in at least one place.

## Simplification Target

1. tag-manager view model
2. tag-select view model
3. presentational components
4. one shared tag list or picker model, not several near-duplicates

## Concrete Work

1. Extract fetching and mutation logic out of `TagManager.tsx`.
2. Normalize tag payloads at the API boundary.
3. Keep relation editing isolated from generic browsing.
4. Collapse the three tag picking or selection surfaces into one reusable list model with mode-specific shells.
5. Keep all namespace parsing generic and stop leaking ingest policy into normal UI parsing.
6. Remove backend maintenance calls from normal page-mount behavior.

## Delete Or Merge

1. Delete frontend-side normalization duplication where backend can own it.
2. Merge tiny tag service files if they do not form a real boundary.
3. Merge duplicate picker implementations.
4. Delete legacy tag-type mapping UI if it no longer reflects the actual backend model.

## Test Target

1. create or rename tag workflow
2. add and remove relation workflow
3. tag picker search and selection workflow
4. one end-to-end tag workflow through inspector or tag manager, not a pile of tiny picker state tests

## Current Audit Findings

1. `TagManager.tsx` is acting as data loader, mutation coordinator, normalization layer, namespace-summary repair hook, and UI.
2. `TagSelectPanel.tsx` is effectively its own mini-application with filtering modes, drag behavior, search, virtualization, and creation behavior.
3. `TagPickerPortal.tsx` and `TagPickerMenu.tsx` duplicate tag loading and grouping behavior instead of sharing one model.
4. Inspector code still reparses stored tag strings because backend response shapes are not normalized enough for direct use.
