# PBI-593: Greenfield frontend persistent inspector and context side surface rebuild

## Priority
P1

## AI-generated caveat
This document is about rebuilding the persistent inspector/context side surface in the new frontend. It exists separately because the current inspector is large, stateful, and tightly coupled to old selection/media wiring.

## Lifecycle
- `Implemented` when the rebuilt inspector and selection surface exist in the new `src/**` tree.
- `Activatable` when the rebuilt grid/selection path and fixture parity harness are ready.
- `Activated` when the rebuilt inspector is the live path.
- `Legacy removed` when the matching legacy inspector path is deleted.

Activation depends on:
- [PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md](./docs/pbis/active-alpha/PBI-590-greenfield-frontend-reference-fixtures-and-parity-harness.md)
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md)
- [PBI-582-greenfield-frontend-controller-boundary-reset.md](./docs/pbis/active-alpha/PBI-582-greenfield-frontend-controller-boundary-reset.md)
- [PBI-587-greenfield-frontend-state-ownership-reset.md](./docs/pbis/active-alpha/PBI-587-greenfield-frontend-state-ownership-reset.md)

## Problem
The current inspector mixes:
- selection state
- metadata loading
- inline editing
- folder/tag actions
- media detail state
- visual layout

That should be rebuilt as a smaller, clearer surface after the rebuilt grid exists.

## Product model to encode
The rebuilt inspector should:
- always be present in grid view
- consume the rebuilt selection state for entity mode
- consume canonical entity details and selection summary shapes
- show current scope context when nothing is selected
- keep selection-driven actions controller-owned
- keep layout and rendering separate from data ownership

Locked behavior:
- entity selection overrides scope mode, but does not control inspector visibility
- scope mode uses the same inspector grammar as entity mode: preview, name, notes, properties, optional sections
- folder and smart-folder notes are real persisted metadata
- system-scope notes are fixed read-only descriptions

The rebuilt inspector should not preserve a separate legacy preview implementation if the same visual object already exists in the rebuilt grid or viewer surface.
If the inspector preview is just the same rounded media preview with different surrounding controls, it should share that preview primitive.
Its styling should follow the CSS architecture contract in [PBI-594-greenfield-frontend-css-architecture-contract.md](./docs/pbis/active-alpha/PBI-594-greenfield-frontend-css-architecture-contract.md) instead of rebuilding inspector-specific copies of shared visual primitives.

## Start gate
This PBI may start only when:
- [PBI-592-greenfield-frontend-grid-screen-rebuild.md](./docs/pbis/active-alpha/PBI-592-greenfield-frontend-grid-screen-rebuild.md) is `Activated`
- canonical selection, entity-details, and selection-summary behavior is stable
- preview and media primitive-family decisions for the rebuilt slice are already locked

## Next rule
Do not start [PBI-585-greenfield-frontend-media-consumption-reset.md](./docs/pbis/active-alpha/PBI-585-greenfield-frontend-media-consumption-reset.md) until this PBI is `Activated`.

## Acceptance criteria
- the rebuilt inspector uses the rebuilt selection state
- the rebuilt inspector is persistent in grid view
- no-selection mode shows current scope context instead of an empty state
- metadata and selection summary behavior are stable
- tag/folder/notes/rating/source-url actions remain correct
- parity is confirmed against the reference harness
- the rebuilt inspector no longer depends on the legacy inspector architecture
- preview and row primitives that are equivalent to rebuilt grid/sidebar/UI primitives are shared instead of rebuilt separately
- finishing this PBI means the rebuilt inspector slice becomes the active default path before the next rebuilt live slice starts
- temporary TODOs are allowed only for cross-PBI boundaries already named in the dependency list; they do not allow the next Track A PBI to start early

## Tests
- fixture rendering tests for single-item, multi-item, collection, and virtual-selection states
- interaction tests for the main inspector actions
- parity checklist and visual confirmation notes

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
