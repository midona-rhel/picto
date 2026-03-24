# PBI-594: Greenfield frontend CSS architecture contract

## Priority
P1

## AI-generated caveat
This document is the styling contract for the rebuilt frontend. It exists to stop the rebuild from copying legacy CSS structure into the new `src/**` tree. It is a binding architecture rule, not a later cleanup pass.

## Lifecycle
- `Implemented` when the styling model, ownership rules, and file-layout rules are written clearly enough to execute without inventing styling architecture during implementation.
- `Activatable` when the first rebuilt frontend slice can follow this styling model without depending on legacy CSS paths.
- `Activated` when the first rebuilt live slice uses this styling model by default.
- `Legacy removed` when rebuilt slices no longer depend on copied or mirrored legacy CSS structure.

Activation depends on:
- [PBI-588-greenfield-frontend-architecture-contract-reset.md](./docs/pbis/active-alpha/PBI-588-greenfield-frontend-architecture-contract-reset.md)
- [PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md](./docs/pbis/active-alpha/PBI-589-greenfield-frontend-legacy-quarantine-and-workspace-reset.md)

## Problem
The legacy frontend styling is too large, too feature-owned, and too duplicated to serve as the structure for the rebuild.

Current styling problems:
- large feature stylesheets own visuals that should belong to shared primitives
- repeated UI surfaces have separate CSS definitions even when they are effectively the same thing
- `globals.css` and feature CSS carry too much mixed responsibility
- styling structure follows legacy feature history more than actual UI ownership
- AI-assisted work tends to copy old CSS into new slices unless the styling model is locked early

## Fixed styling rules
Use this styling model for the rebuilt frontend:
- `src/app/globals.css` is for true globals only
- `src/shared/styles/tokens.css` owns the design tokens
- shared primitives own their own CSS Modules under `src/shared/**`
- feature CSS Modules own composition and surface layout only
- no rebuilt live slice should import legacy CSS files

Use this ownership rule:
- globals own reset, root layout, base text/background, and true global defaults only
- tokens own colors, spacing, radii, shadows, typography scale, motion, and layers
- shared primitives own repeated visuals such as rows, panels, buttons, popup shells, and media frames
- features own placement, composition, and surface-specific layout only

Use this consolidation rule:
- if two rebuilt UI elements look and behave the same, they should not have separate CSS definitions
- prefer one configurable primitive over several copied style variants
- do not preserve separate legacy CSS because the old code lived in different features

Examples:
- sidebar rows should share one row styling family
- picker and popup panels should share the same popup surface styling family when the interaction model is the same
- grid preview tiles, inspector previews, and similar rounded media shells should share one media-frame styling family when the visual object is the same

## Required file shape
- `src/app/globals.css`
- `src/shared/styles/tokens.css`
- `src/shared/styles/**` only for small shared styling helpers if truly needed
- `src/shared/**/**/*.module.css` for shared primitives
- `src/features/**/**/*.module.css` for feature composition only

## Required token shape
At minimum, tokens should cover:
- app/background/surface colors
- text and border colors
- spacing scale
- radius scale
- shadow scale
- typography scale
- motion durations/easing
- z-index/layer values

## CSS rules for rebuilt slices
- do not introduce giant feature stylesheets
- do not use deep selector chains when a primitive boundary should own the style
- do not re-create button, row, panel, or preview styling inside features
- do not copy legacy CSS into the rebuilt slice except as an explicit short-lived reference during implementation
- if a style repeats twice, consider moving it into a shared primitive

## Acceptance criteria
- the styling model is explicit before more rebuilt live slices start
- rebuilt slices use tokens, shared primitives, and component-owned CSS Modules
- `globals.css` is limited to true globals
- feature CSS is mostly layout/composition, not repeated primitive styling
- duplicated styling is reduced instead of being recreated under new paths
- later frontend PBIs can reference this document instead of inventing CSS ownership per slice

## Tests
- styling ownership review checklist for rebuilt slices
- spot checks proving rebuilt slices do not import legacy CSS
- before/after CSS size and duplication audits where relevant

This PBI must follow the cross-layer naming contract in [PBI-572-cross-layer-naming-contract.md](./docs/pbis/active-alpha/PBI-572-cross-layer-naming-contract.md).
This PBI must follow the cross-layer testing rules in [PBI-579-cross-layer-testing-rules.md](./docs/pbis/active-alpha/PBI-579-cross-layer-testing-rules.md).
This PBI must follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).
