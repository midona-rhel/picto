# Frontend Legacy Register

Date: 2026-03-07
Related plan: `docs/frontend-topology-enforcement-and-legacy-deletion-plan-2026-03-07.md`

This file tracks frontend paths that are transitional, obsolete, or blocked on
deletion.

Statuses:

1. `transitional`
2. `delete-now`
3. `merge-then-delete`
4. `blocked`

## Ledger

| Path | Category | Reason | Replacement / Target Owner | Delete Condition | Owner PBI | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `src/components/` | catch-all legacy domain tree | Mixes shared primitives with domain-owned screens/components | `src/features/*` for domain code, `src/shared/components/*` for real primitives | After every child path is classified and migrated | `PBI-401`, `PBI-406`, `PBI-407` | transitional |
| `src/App.tsx` | root shell residue | App shell and composition root still live directly under `src/` | `src/app/` | After app shell move is complete and entrypoint imports are updated | `PBI-401` | transitional |
| `src/detail.tsx` | root entrypoint residue | Valid entrypoint, but should live under `src/entrypoints/` | `src/entrypoints/detail.tsx` | After entrypoint folder migration lands | `PBI-401` | transitional |
| `src/library-manager.tsx` | root entrypoint residue | Valid entrypoint, but should live under `src/entrypoints/` | `src/entrypoints/library-manager.tsx` | After entrypoint folder migration lands | `PBI-401` | transitional |
| `src/main.tsx` | root entrypoint residue | Valid entrypoint, but should live under `src/entrypoints/` | `src/entrypoints/main.tsx` | After entrypoint folder migration lands | `PBI-401` | transitional |
| `src/settings.tsx` | root entrypoint residue | Valid entrypoint, but should live under `src/entrypoints/` | `src/entrypoints/settings.tsx` | After entrypoint folder migration lands | `PBI-401` | transitional |
| `src/subscriptions.tsx` | root entrypoint residue | Valid entrypoint, but should live under `src/entrypoints/` | `src/entrypoints/subscriptions.tsx` | After entrypoint folder migration lands | `PBI-401` | transitional |

## Notes

1. This file starts with seed entries only.
2. `PBI-406` should expand this to a full classification pass over current frontend files.
