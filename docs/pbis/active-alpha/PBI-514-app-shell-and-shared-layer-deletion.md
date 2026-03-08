# PBI-514: App shell and shared-layer deletion

## Priority
P0

## Problem
The app shell and `shared` layer still contain broad orchestration, duplicate portals, and business helpers that are not truly shared primitives.

## Goal
Turn the app shell into composition and delete the shared junk-drawer pattern.

## Implementation
1. `App.tsx` becomes layout and composition only.
2. Split command palette, shell controls, and native listeners into focused modules.
3. Remove duplicate picker portals and repeated feature-local fetch/group logic.
4. Keep only real reusable primitives in `shared`.

## Acceptance Criteria
1. App shell is mostly composition.
2. Duplicate picker/search/grouping surfaces are collapsed.
