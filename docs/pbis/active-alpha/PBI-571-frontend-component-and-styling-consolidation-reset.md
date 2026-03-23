# PBI-571: Frontend component and styling consolidation reset

## Priority
P1

## AI-generated caveat
This document is based on an in-repo audit of the current frontend structure, repeated components, and stylesheet footprint. It is intentionally concrete and decision-complete, but it is still AI-generated planning. The implementing engineer should simplify further where that preserves the same visual product.

## Problem
The frontend is visually strong and feature-rich, but the internal UI structure is still far messier than it needs to be.

Current problems:
- component patterns are re-created across multiple features instead of being canonicalized
- very large feature-specific stylesheets own styling that should belong to shared primitives
- CSS volume is materially larger than the product requires
- comments and naming still reflect stale structure in several places
- repeated logic and repeated shells make the UI harder to maintain than the current product justifies
- weak UI abstraction boundaries make later architectural cleanup harder because visual structure and feature wiring are too entangled

This PBI is about UI structure and styling consolidation while preserving the current visual product closely.

## Product model to encode
The frontend UI should reflect these truths:
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

Those canonical implementations are a real abstraction layer. They should become the normal way features render shared UI patterns, not an optional library sitting next to more one-off copies.

### 3. Centralize styling around tokens and primitives
The styling model should be:
- global tokens and true globals only
- shared layout and visual primitives
- component-owned CSS Modules where appropriate
- minimal feature-specific overrides

Do not keep pushing shared styling concerns into giant feature sheets.

### 4. Shrink CSS materially
The target is not “rename CSS”.

The target is:
- less CSS
- fewer duplicate selectors
- fewer giant feature stylesheets
- clearer ownership

## Known first-review targets
The first large stylesheets to review and shrink are:
- inspector panel
- subscription groups panel
- tag manager
- sidebar
- settings
- filter bar
- globals

These are the places where duplicated visual logic is most likely hiding today.

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

Feature code should own:
- behavior specific to that feature
- only the styling that is genuinely feature-unique

### Styling system
Use:
- global tokens
- shared layout helpers
- shared interaction-state styling
- component-level CSS Modules

Reduce `globals.css` to true globals only.

## Implementation changes
- audit repeated components across sidebar, inspector, settings, tags, subscriptions, viewer, and shared surfaces
- extract one canonical implementation per repeated pattern
- centralize design tokens for spacing, typography, colors, surface states, and interaction states
- reduce or split giant feature stylesheets where shared primitives should own the styling
- remove dead comments, stale naming, and duplicated UI logic while preserving current behavior
- keep CSS Modules where appropriate, but make them smaller and more component-owned instead of feature-mega-files

## Relationship to PBI-570
PBI-570 is the frontend architecture and backend-boundary reset.

PBI-571 is the UI composition and styling reset.

Do not merge them into one implementation blob. The architecture work and the visual-structure work should be separately reviewable even though they support each other.

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Acceptance criteria
This PBI is complete only when:
- current visuals remain effectively the same
- repeated component patterns have canonical implementations
- giant feature stylesheets are materially reduced
- `globals.css` is reduced to true globals
- naming and comments in the affected UI surface are clearer and current
- shared UI primitives are the default abstraction layer for repeated patterns
- duplicated styling and duplicated UI logic are materially reduced

## Tests
Required tests:
- visual smoke/regression tests for main surfaces
- component contract tests for extracted shared primitives
- interaction parity checks for sidebar, inspector, settings, tags, subscriptions, and viewer
- CSS size and usage audit before vs after
- duplication audit for canonicalized component families

## Adjacent cleanup expected during implementation
While implementing this PBI, also remove:
- dead UI comments
- stale naming that reflects deleted architecture
- duplicate one-off component variants that only differ cosmetically
