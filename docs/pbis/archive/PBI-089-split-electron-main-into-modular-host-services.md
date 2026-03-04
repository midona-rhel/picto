# PBI-089: Split Electron main into modular host services

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. `/Users/midona/Code/imaginator/electron/main.mjs` is 1425 lines.
2. The same file currently mixes unrelated responsibilities:
   - media protocol parsing/streaming + thumbnail regeneration
   - blurhash backfill scheduling
   - window lifecycle/menu management
   - generic invoke IPC and many specialized IPC handlers
   - reverse image search browser automation
   - library create/open/switch/rename/relocate workflows

## Problem
The host runtime is a monolith. This increases restart fragility, slows iteration, and makes ownership boundaries unclear between transport, protocol serving, windowing, and application services.

## Scope
- `/Users/midona/Code/imaginator/electron/main.mjs`
- new modules under `/Users/midona/Code/imaginator/electron/`:
  - `ipc/`
  - `protocol/`
  - `services/`
  - `windows/`

## Implementation
1. Reduce `main.mjs` to bootstrap/composition only.
2. Extract:
   - media protocol handler into `protocol/media.mjs`
   - blurhash/thumbnail background loop into `services/mediaMaintenance.mjs`
   - IPC handler groups into `ipc/*.mjs` (invoke/window/dialog/library/clipboard/search/drag)
   - window/menu lifecycle into `windows/*.mjs`
3. Add module-level unit tests for parse/validation-heavy utilities (media URL parsing, range parsing, input validation).
4. Add startup smoke test to ensure all handlers register exactly once.

## Acceptance Criteria
1. `main.mjs` becomes a thin bootstrap file with clear module wiring.
2. IPC and protocol logic are isolated by concern.
3. Behavior parity is preserved for library operations, media serving, and window interactions.

## Test Cases
1. App startup and library boot under dev/prod modes.
2. Media protocol file/thumb fetch (including range and 404 paths).
3. Library switch/rename/relocate flows.
4. Reverse image search and native drag flow still functional.

## Risk
Medium. Broad refactor in host process; low algorithmic risk but high surface area.

