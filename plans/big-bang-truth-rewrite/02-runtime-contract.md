# Runtime Contract

## Purpose

Define the only supported live frontend/backend dataflow.

## Current Truth

- Runtime state is split across mutation receipts, task state, legacy events, refresh helpers, and fallback polling.
- Components and stores still know too much about how to invalidate unrelated domains.

## Target Truth

- Frontend boots from `RuntimeSnapshot`.
- Backend emits only:
  - `runtime/mutation_committed`
  - `runtime/task_upserted`
  - `runtime/task_removed`
  - library open/close/switch events
- Frontend derives stale resources locally from receipt facts.
- Polling exists only as recovery fallback.

## Rename Map

- `state-changed`, `sidebar-invalidated`, `grid-snapshot-invalidated` -> deleted
- `taskRuntimeStore`, `eventBridge` style feature listeners -> deleted
- `MutationReceipt.invalidate` transitional hints -> deleted after fact coverage is complete

## Delete List

- Delete compatibility events in `core/src/events.rs`.
- Delete feature-owned invalidation helpers in renderer code.
- Delete normal-operation watchdog refresh logic.

## DTOs and Commands Involved

- `MutationReceipt`
- `RuntimeTask`
- `TaskUpsertedEvent`
- `TaskRemovedEvent`
- `get_runtime_snapshot`

## Workflows

- App boot -> fetch snapshot -> subscribe to runtime events -> derive stale resources -> refetch only needed views.
- Mutation command succeeds -> receipt reports affected entities, tags, folders, smart folders, scopes, and sidebar counts.
- Long-running job -> task upsert updates progress -> task removed clears it.

## Acceptance Criteria

- Renderer no longer depends on domain-specific invalidation events.
- No component manually refreshes sidebar, grid, inspector, and tags together.
- Runtime state can be explained from snapshot + receipt facts + tasks only.
