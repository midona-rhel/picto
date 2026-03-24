# PBI-589: Greenfield frontend legacy quarantine and workspace reset

## Priority
P1

## AI-generated caveat
This document is about separating the old frontend from the new one. It is not a generic cleanup PBI. The point is to stop interleaving old and new architecture in the same active `src/**` tree.

## Lifecycle
- `Implemented` when the current frontend implementation is moved out of `src/**` into a clearly named legacy workspace and the new `src/**` tree can host the rebuilt frontend.
- `Activatable` when the legacy frontend is still available for reference/parity but no longer blocks creation of the new active source tree.
- `Activated` when the app is built from the new `src/**` tree and the legacy frontend is reference-only.
- `Legacy removed` when a rebuilt slice has replaced its matching legacy slice and that legacy code is deleted.

Activation depends on:
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)

## Problem
The current frontend rewrite work is being attempted inside the same `src/**` tree as the old frontend. That causes:
- constant churn between old and new architecture
- accidental reuse of old stores, old hooks, and old controller assumptions
- active route breakage during migration
- AI-assisted implementation bias toward patching old code instead of replacing it

## Product model to encode
The codebase should have:
- one explicit legacy frontend workspace
- one explicit new active frontend workspace
- clear rules about what the new frontend may and may not import from legacy

The legacy frontend is for:
- behavior reference
- visual reference
- parity checking
- extracting product intent

The legacy frontend is not for:
- continued architecture work
- new feature development
- being part of the new runtime path

## Required shape
- move the current frontend implementation out of `src/**`
- place it under a clearly named legacy path outside `src/**`, such as `legacy/frontend/**`
- create a clean `src/**` root for the rebuilt frontend
- document the legacy-to-new mapping by surface
- keep the legacy frontend runnable or inspectable enough for parity/reference work

## Rules
- the new frontend may not depend on legacy store/controller/runtime modules in its normal runtime path
- legacy code is reference-only except for blocker fixes needed to keep parity/reference usable
- no broad new architecture work should be done in the legacy tree
- if a rebuilt slice needs product understanding from legacy, copy the behavior intentionally; do not import the legacy architecture wholesale

## Acceptance criteria
- the old frontend is no longer the active implementation in `src/**`
- the new frontend has a clean active source tree
- the legacy frontend has a clearly named home outside `src/**`
- import boundaries between new and legacy are explicit
- the team can rebuild a surface without dragging old runtime/state architecture into it

## Tests
- workspace/build check proving the new `src/**` tree is the active frontend
- boundary grep/review proving the rebuilt frontend is not importing legacy runtime/store/controller paths as product dependencies
- documentation mapping old surface locations to their legacy paths

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
