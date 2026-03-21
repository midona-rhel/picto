# Frontend Structure

## Purpose

Define the only allowed frontend feature surfaces after the rewrite.

## Current Truth

- Frontend functionality mostly works, but ownership is splintered across shared controllers, portals, duplicated components, and oversized orchestration modules.

## Target Truth

- Frontend features are:
  - app-shell
  - sidebar
  - grid
  - inspector
  - tags
  - folders
  - smart-folders
  - subscriptions
  - settings
- Shared code is limited to true UI primitives and low-level helpers.
- Feature state is thin and presentation-focused.

## Rename Map

- `shared/controllers/*` -> deleted or folded into feature-local code
- `FlowsWorking` target naming -> subscription-group UI

## Delete List

- Delete shared controller architecture.
- Delete duplicate picker portals.
- Delete frontend-visible PTR surfaces.
- Delete feature code that reparses backend semantics.

## DTOs and Commands Involved

- `src/platform/api.ts`
- feature-local hooks and view models
- no new public command layer beyond the central API surface

## Workflows

- App shell boots runtime and layout.
- Sidebar selects scope.
- Main view routes to grid, tags, subscriptions, settings, or duplicates.
- Inspector reflects current selection and sends commands only.

## Acceptance Criteria

- A feature can be understood from one view model plus one component tree.
- No feature depends on a shared controller to rename API calls.
- The active frontend module list matches the target list above.
