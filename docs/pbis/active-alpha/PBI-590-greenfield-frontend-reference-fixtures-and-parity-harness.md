# PBI-590: Greenfield frontend reference fixtures and parity harness

## Priority
P1

## AI-generated caveat
This document is about parity/reference infrastructure for the frontend rebuild. It exists because the frontend is being rebuilt from scratch and needs explicit visual and behavior checkpoints instead of guesswork.

## Lifecycle
- `Implemented` when the rebuild has explicit fixtures, parity checklists, and reference surfaces for the first rebuilt slices.
- `Activatable` when the first rebuilt slice can be compared against a stable reference without relying on the live app state by hand.
- `Activated` when every rebuilt slice uses this parity harness before activation.
- `Legacy removed` when a replaced slice no longer needs its legacy reference assets or harness support.

Activation depends on:
- [PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md](./docs/pbis/active-alpha/PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md)

## Problem
Rebuilding the frontend from scratch will break things temporarily. That is acceptable, but only if there are explicit checkpoints to confirm that the rebuilt surface still matches the old product where it matters.

Without a reference harness, the rebuild becomes:
- subjective
- easy to drift visually or behaviorally
- hard to verify when working against dummy data

## Product model to encode
Each rebuilt slice should have:
- a legacy reference to compare against
- frozen fixture data for rendering without backend dependency
- a parity checklist for visual and behavioral confirmation

## Required shape
- fixture payloads for the first major surfaces:
  - sidebar tree
  - grid page
  - entity details / inspector state
  - selection summary
  - media/viewer asset states where needed
- one reference harness or lab path for rendering rebuilt surfaces with fixture data
- one parity checklist per rebuilt slice
- one screenshot or visual confirmation workflow per rebuilt slice

## Rules
- fixture data is for rebuilding and visual verification, not the long-term data layer
- parity should check:
  - structure
  - counts
  - ordering
  - empty/loading/error states
  - major interactions
- parity does not require preserving bad legacy architecture
- when visuals intentionally differ, the rebuilt slice must state why before activation

## Acceptance criteria
- the first rebuilt slices can be rendered against stable fixture data
- the team can compare legacy vs rebuilt sidebar/grid/inspector without depending on the full live app
- each rebuilt slice has a concrete parity checklist
- activation of rebuilt slices requires parity confirmation

## Tests
- fixture rendering smoke tests
- parity harness smoke tests
- per-slice parity notes or screenshot artifacts

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
