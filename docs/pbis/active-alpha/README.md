# Active PBIs

This directory still contains both active work and historical greenfield-reset notes.

Frontend status:
- the frontend was rebuilt and cleaned up in place
- the API/controller/state/runtime cleanup slices are largely implemented
- shell, grid, and inspector are the live path

Frontend follow-ups still worth treating as genuinely open:
- [PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md](./docs/pbis/active-alpha/PBI-595-greenfield-frontend-manager-navigation-and-surface-reset.md)
- [PBI-596-greenfield-random-active-image-view-contract.md](./docs/pbis/active-alpha/PBI-596-greenfield-random-active-image-view-contract.md)
- [PBI-599-context-menu-action-parity.md](./docs/pbis/active-alpha/PBI-599-context-menu-action-parity.md)

Backend/runtime and metadata PBIs in this folder remain active when they still describe current repo work. The old reset-program sequencing documents should be read as historical notes unless they have been explicitly refreshed.

Persistence and sync track (2026-07-17 audit):
- [PBI-601-local-persistence-crash-safety-hardening.md](./docs/pbis/active-alpha/PBI-601-local-persistence-crash-safety-hardening.md) — P1 prerequisite: atomic blob writes, transaction wraps, durability pragmas
- [PBI-602-multi-device-sync-architecture.md](./docs/pbis/active-alpha/PBI-602-multi-device-sync-architecture.md) — append-only oplog over dumb object storage (Google Drive first)
