# PBI-571: Frontend shared component and styling system reset

## Priority
P1

## AI-generated caveat
This document is about the shared UI system specifically. It is not the whole frontend re-engineer. The goal is to make shared UI primitives and styling ownership explicit after the frontend architecture PBIs have stabilized the surrounding layers.

## Lifecycle
- `Implemented` when the shared UI primitives, component consolidation, and styling cleanup exist in code.
- `Activatable` when [PBI-586-greenfield-frontend-feature-module-architecture-reset.md](./docs/pbis/active-alpha/PBI-586-greenfield-frontend-feature-module-architecture-reset.md) has stabilized the feature/module ownership enough that UI cleanup is not fighting ongoing architectural churn.
- `Activated` when the live frontend surfaces use the consolidated component/styling system by default.
- `Legacy removed` when replaced duplicate components and giant obsolete styling paths are deleted.

Activation depends on:
- [PBI-586-greenfield-frontend-feature-module-architecture-reset.md](./docs/pbis/active-alpha/PBI-586-greenfield-frontend-feature-module-architecture-reset.md)

## Problem
The frontend is visually strong and feature-rich, but the internal UI structure is still far messier than it needs to be.

Current problems:
- component patterns are re-created across multiple features instead of being canonicalized
- very large feature-specific stylesheets own styling that should belong to shared primitives
- CSS volume is materially larger than the product requires
- comments and naming still reflect stale structure in several places
- repeated logic and repeated shells make the UI harder to maintain than the current product justifies
- weak UI abstraction boundaries make later architectural cleanup harder because visual structure and feature wiring are too entangled

## Product model to encode
The shared UI system should reflect these truths:
- current visuals and interaction feel should remain effectively the same
- shared visual patterns should be implemented once
- styling should be driven by tokens, primitives, and small component-owned styles
- giant feature stylesheets should shrink materially
- component families should be obvious and reusable
- visual primitives should form a stable UI layer that feature code composes instead of re-creating

## Locked decisions

### 1. Preserve current visuals
This is not a redesign.

The goal is:
- keep current product visuals closely
- keep current interaction behavior
- improve structure, reuse, and maintainability

### 2. Collapse duplication aggressively
If multiple features implement effectively the same UI pattern, extract one canonical implementation.

This applies to:
- inspector rows
- panel headers
- picker layouts
- tag rows and chips
- context menu patterns
- preview card shells
- repeated button and icon-button variants

### 3. Centralize styling around tokens and primitives
The styling model should be:
- global tokens and true globals only
- shared layout and visual primitives
- component-owned CSS Modules where appropriate
- minimal feature-specific overrides

### 4. Shrink CSS materially
The target is:
- less CSS
- fewer duplicate selectors
- fewer giant feature stylesheets
- clearer ownership

### 5. Rebuild in small verified slices
This PBI must not be executed as one broad frontend sweep.

The required rollout shape is:
- migrate one surface family at a time
- finish its local shared primitives and styling ownership
- get visual confirmation that it still looks and behaves the same
- only then move to the next surface

The default starting slice is:
- sidebar and all sidebar-owned items/components

## Required frontend UI shape

### Shared primitives
Create or consolidate clear shared primitives for:
- visual shells
- section/panel headers
- field rows and property rows
- icon/button primitives
- picker/list rows
- media card and preview containers
- modal and overlay shells

### Feature composition
Features should compose those primitives instead of redefining them.

### Styling system
Use:
- global tokens
- shared layout helpers
- shared interaction-state styling
- component-level CSS Modules

Reduce `globals.css` to true globals only.

## Implementation changes
- execute the work in patch-sized surface slices, not one broad refactor
- start with sidebar and sidebar-owned items/components
- after each slice, do visual parity confirmation before moving to the next surface
- then continue through inspector, settings, tags, subscriptions, viewer, and other shared surfaces
- audit repeated components across each slice before extracting shared primitives from it
- extract one canonical implementation per repeated pattern
- centralize design tokens for spacing, typography, colors, surface states, and interaction states
- reduce or split giant feature stylesheets where shared primitives should own the styling
- remove dead comments, stale naming, and duplicated UI logic while preserving current behavior
- keep CSS Modules where appropriate, but make them smaller and more component-owned instead of feature-mega-files

## Acceptance criteria
- current visuals remain effectively the same
- repeated component patterns have canonical implementations
- giant feature stylesheets are materially reduced
- `globals.css` is reduced to true globals
- naming and comments in the affected UI surface are clearer and current
- shared UI primitives are the default abstraction layer for repeated patterns
- duplicated styling and duplicated UI logic are materially reduced
- each migrated surface was completed in a bounded slice with visual parity checked before the next slice started

## Tests
- visual smoke/regression tests for main surfaces
- component contract tests for extracted shared primitives
- interaction parity checks for sidebar, inspector, settings, tags, subscriptions, and viewer
- CSS size and usage audit before vs after
- duplication audit for canonicalized component families
- per-slice visual confirmation notes or artifacts, starting with the sidebar slice

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
