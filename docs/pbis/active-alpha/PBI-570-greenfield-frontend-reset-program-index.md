# PBI-570: Frontend reset program closure note

## Status
Closed as a planning artifact.

## What changed
The repo did not end up following the original “move the old frontend out of `src/**` and rebuild a new frontend elsewhere” strategy.

Instead, the frontend was cleaned up and activated in place:
- transport was split into smaller `src/platform/**` modules
- controller/state/runtime ownership was tightened in the live tree
- shell, grid, and inspector are now the active product path

## How to read the remaining frontend PBIs
- Treat the old reset-program sequencing as historical context only.
- Use individual PBIs for still-open visible behavior work.
- The main frontend follow-ups still worth treating as open are:
  - [PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md](./docs/pbis/active-alpha/PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md)
  - [PBI-596-greenfield-random-active-image-view-contract.md](./docs/pbis/active-alpha/PBI-596-greenfield-random-active-image-view-contract.md)
  - [PBI-599-context-menu-action-parity.md](./docs/pbis/active-alpha/PBI-599-context-menu-action-parity.md)

## Repo truth
The active frontend architecture is now in-place and controller/state/runtime driven. Do not use this document as an instruction to restart a greenfield frontend migration.
