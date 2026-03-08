# Frontend Subscriptions

Current footprint: `src/features/subscriptions`, about 8 files and about 1.9k lines

## What This Should Own

1. subscriptions and flows UI
2. form and modal state
3. runtime progress presentation

## What This Should Not Own

1. broad backend refresh orchestration
2. duplicated progress models

## Why It Is Too Complicated

1. `FlowsWorking.tsx` and `SubscriptionsWindow.tsx` still do too much.
2. Data loading, notifications, runtime progress, modal state, query editing, and CRUD actions are still too tangled.
3. The feature reacts to broad runtime changes by reloading too much.

## Simplification Target

1. one subscriptions view model
2. one flows view model
3. mostly presentational components

## Concrete Work

1. Extract data and mutation hooks from the UI components.
2. Consume runtime task projections directly instead of rebuilding parallel state.
3. Replace broad `loadData()` refreshes with narrower invalidation.

## Delete Or Merge

1. Delete duplicate refresh logic.
2. Merge tiny subscription UI helpers into a dedicated view-model module.

## Test Target

1. create flow, add subscription, add query, run, stop workflow
2. error-state workflow
3. progress rendering workflow
