# Backend Subscriptions

Current footprint: about 7 files and about 6.6k lines

## What This Should Own

1. subscription CRUD
2. flow CRUD and scheduling
3. query execution
4. gallery-dl orchestration
5. progress reporting

## What This Should Not Own

1. generic import logic
2. global runtime state storage
3. renderer-specific progress formatting

## Why It Is Too Complicated

1. This is one of the largest backend domains and one of the least disciplined.
2. Flow management, subscription CRUD, sync orchestration, credential rules, and subprocess handling are still too entangled.
3. The domain is acting like three systems pretending to be one.

## Simplification Target

1. subscription config service
2. flow scheduler or orchestrator
3. query run engine
4. gallery-dl adapter
5. progress adapter to runtime tasks

## Concrete Work

1. Split flow CRUD from flow execution.
2. Split subscription CRUD from query-run orchestration.
3. Move all subprocess details behind a gallery-dl adapter layer.
4. Stop carrying a separate custom progress/event story if runtime tasks already exist.

## Delete Or Merge

1. Delete legacy compatibility event handling once runtime tasks fully cover progress.
2. Merge tiny helper functions into the proper orchestrator instead of scattering them.

## Test Target

1. create flow, create subscription, add query, run, stop, reset workflow
2. credential injection workflow
3. gallery-dl failure mapping workflow
