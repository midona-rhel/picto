# Frontend Grid

Current footprint: `src/features/grid`, about 82 files and about 18.3k lines

## What This Should Own

1. grid rendering
2. grid interaction state
3. query and pagination adapters for the renderer
4. viewer handoff from the grid

## What This Should Not Own

1. app-shell logic
2. global runtime policy
3. duplicated backend query semantics

## Why It Is Too Complicated

1. This is by far the largest frontend feature.
2. The grid owns rendering, selection, transitions, caching, QoS, query brokering, detail view, drag behavior, and more.
3. It is currently too large to reason about safely as one feature.

## Simplification Target

1. grid rendering
2. grid interaction model
3. grid data model
4. viewer handoff

## Concrete Work

1. Keep data loading, interaction logic, and rendering in separate sub-areas with strict boundaries.
2. Move shared feature logic out of giant components into feature-local models, not `shared`.
3. Avoid re-implementing backend query behavior on the frontend.

## Delete Or Merge

1. Delete duplicate hooks that only shuffle state between grid modules.
2. Merge tiny grid helpers if they are splitting one algorithm across too many files.

## Test Target

1. a few real interaction workflows
2. a few pure-function tests for layout or selection math
3. no vanity tests that only enforce file size or internal layering opinions
