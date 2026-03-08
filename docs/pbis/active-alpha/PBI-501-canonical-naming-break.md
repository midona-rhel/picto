# PBI-501: Canonical naming break

## Priority
P0

## Problem
The codebase still teaches contributors the wrong product model through names like `flow`, `sibling`, `parent`, and logical `file` references that actually mean media entities.

## Goal
Make active docs, UI, and non-generated frontend code use truthful product names only.

## Required Naming
1. `flow` -> `subscription_group`
2. `sibling` -> `alias`
3. `parent` -> `implication`
4. logical `file` -> `media_entity` where it means the library item
5. visible `PTR` references removed
6. collection-member hiding described as visibility/projection, never lifecycle

## Implementation
1. Rename route, store, component, view-model, and user-facing surfaces.
2. Update active docs and settings/runtime UI copy.
3. Leave generated or backend compatibility names only where migration is still in progress.

## Acceptance Criteria
1. Old names are gone from active frontend route/store/component surfaces.
2. Docs and UI use the new names exclusively.
