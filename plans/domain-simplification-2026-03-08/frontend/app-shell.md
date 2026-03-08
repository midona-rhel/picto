# Frontend App Shell

Current footprint: `src/app`, about 4 files and about 917 lines

## What This Should Own

1. window shell composition
2. global shortcut wiring
3. startup wiring
4. top-level view composition

## What This Should Not Own

1. domain data fetching
2. feature orchestration details
3. progress or invalidation policy

## Why It Is Too Complicated

1. `App.tsx` is still a large composition and orchestration surface.
2. Global shortcuts, command palette setup, view composition, and shell layout still live too close together.
3. This is the classic "root component does everything because it can" problem.

## Simplification Target

1. `App.tsx` becomes mostly composition
2. startup logic lives in one bootstrap hook
3. shell-only view model is separate from feature state

## Concrete Work

1. Extract command-palette model from `App.tsx`.
2. Extract shell layout state from feature state.
3. Keep global hotkeys and native startup wiring explicit, not mixed into everything else.

## Delete Or Merge

1. Delete shell code that belongs in feature hooks.
2. Merge tiny shell helpers if they only support one call site.

## Test Target

1. one shell integration test for route/view switching
2. one startup wiring test
3. one global shortcut smoke test
