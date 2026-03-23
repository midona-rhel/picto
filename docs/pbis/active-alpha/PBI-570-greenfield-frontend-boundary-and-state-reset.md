# PBI-570: Greenfield frontend boundary and state reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current frontend/backend boundary, controller layer, and feature-level state usage. It is intentionally concrete and decision-complete, but it is still AI-generated planning. The implementing engineer should simplify further where that preserves the same behavior and boundary.

## Problem
The frontend works well today, but the way it talks to the backend and organizes state is still messier than the current product model justifies.

Current problems:
- feature code still carries too much knowledge of backend transport and backend semantics
- several surfaces re-implement equivalent intent in different hooks or feature modules
- media consumption is still partially shaped by backend implementation details
- grid/inspector/sidebar logic is more scattered than it should be
- the frontend boundary is cleaner than before, but not yet reduced to one obvious architecture
- the frontend does not yet depend on a small enough application-facing boundary to make later replacement straightforward

This PBI is the frontend architecture-half of the reset. It is about state interaction and application boundaries, not about redesigning the UI.

## Product model to encode
The frontend should reflect these application truths:
- feature code talks to controllers or view-model hooks only
- the main grid is driven by one typed entity-view query model
- media is consumed through stable backend-generated media URLs
- direct visible updates happen locally where safe
- authoritative backend state changes reconcile the rest
- the visual product should remain effectively the same
- controllers should depend on one backend adapter contract, not on a concrete local transport implementation

## Locked decisions

### 1. Controllers only
Feature code should not call backend APIs directly.

The allowed frontend boundary is:
- controllers
- domain state/view-model hooks
- typed runtime/state-change appliers
- media URL helpers built on the new media delivery service

Feature code should not talk directly to:
- raw platform invoke layer
- ad hoc path resolvers
- transport-shaped command names
- duplicated read APIs for equivalent entity operations

### 1a. Controllers depend on one backend adapter contract
Controllers should talk to one typed backend adapter interface, for example `AppBackend`, not to a concrete desktop transport implementation.

Rules:
- the adapter exposes application-facing queries, write calls, and media URL helpers
- local IPC, remote HTTP, or any future transport sits behind that adapter
- feature code and view-model hooks do not know or care which transport is active

The important boundary is not “frontend process vs backend process”. The important boundary is “feature code vs stable application contract”.

### 2. One typed entity view query drives the grid
All grid surfaces should be driven by the typed entity-view query model from the backend engine reset.

That includes:
- system scopes
- folders
- collections
- smart folders
- similar/search scopes
- filters, sort, and cursor pagination

### 3. Batch grid-item reads are reconciliation-only
`get_entity_grid_items(entity_hashes)` exists only for:
- targeted reconciliation
- eager insertion/update paths

It is not the main way to drive the grid.

### 4. Media display uses stable media URLs
The viewer, grid, slideshow, strip view, quick look, previews, and drag ghosts should consume stable media URLs or handles from the media delivery layer.

Do not keep frontend logic built around path resolution.

### 5. Visual behavior stays effectively the same
This PBI is not a redesign.

The goal is:
- same product behavior
- same practical look
- cleaner state and boundary architecture

## Required frontend shape

### Controllers
Controllers own:
- backend query and write calls
- eager direct visible updates where safe
- optimistic behavior and reconciliation handoff
- backend-target construction
- domain-specific command deduplication

They should depend on:
- the typed backend adapter contract
- typed controller-local helpers

They should not depend on:
- transport clients directly
- invoke command names
- file/path-shaped backend helpers

### Domain state/view-model hooks
Hooks own:
- composing controller data for a feature
- UI-facing state derivation
- local UI state only

They should not:
- replicate backend command semantics
- call raw backend APIs
- own duplicated domain write logic

### Runtime/state-change layer
The runtime layer owns:
- authoritative backend reconciliation
- targeted refresh application
- cross-surface state settlement after backend state changes

Controllers and runtime should have a clean relationship:
- controller applies direct local visible effect
- backend commits
- runtime applies authoritative reconciliation

## Implementation changes
- remove remaining direct backend access from feature code
- drive all grid surfaces from one typed entity-view query model
- route media display through stable media URLs from the media delivery service
- remove duplicated feature-level state logic where grid, inspector, sidebar, settings, and viewer are independently modeling the same backend intent
- standardize controller/view-model boundaries for grid, inspector, sidebar, viewer, settings, and tags
- keep subscriptions out of this PBI except for general boundary-proofing if necessary; subscription-specific frontend cleanup stays separate

## Relationship to other reset PBIs
- PBI-567 defines the canonical database model
- PBI-568 defines backend query and write semantics
- PBI-569 defines media delivery
- PBI-570 defines how the frontend consumes that backend cleanly
- PBI-571 defines how the frontend UI itself is structurally consolidated

PBI-570 is not the styling PBI. It is the frontend architecture and state-boundary PBI.

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- feature code does not call backend APIs directly
- controllers are the only backend-facing frontend layer
- controllers depend on one backend adapter contract rather than a concrete transport implementation
- the main grid is fully driven by the typed entity-view query model
- reconciliation paths use batch grid-item reads only for targeted updates
- viewer/grid/preview surfaces consume stable media URLs instead of path helpers
- current visual behavior remains effectively unchanged

## Tests
Required tests:
- boundary tests proving feature code does not call backend APIs directly
- controller tests for grid, inspector, sidebar, viewer, and settings interactions
- typed entity-view query flow tests from frontend into backend
- targeted reconciliation tests using batch grid-item reads
- media URL consumption tests in viewer, grid, and preview surfaces
- regression tests proving current visual behavior still works

## Adjacent cleanup expected during implementation
While implementing this PBI, also remove:
- stale feature-level backend comments
- duplicated domain intent spread across multiple hooks
- path-shaped frontend media assumptions
