# Sidebar And Navigation

## Purpose

Define navigation and published tree structure without treating sidebar as a business domain.

## Current Truth

- Sidebar mixes system scopes, counts, folders, smart folders, drag/drop, and runtime refresh policy.
- Navigation state is partially clean, but read-model ownership is still blurry.

## Target Truth

- Sidebar is a published read model.
- It contains:
  - system scopes
  - folders
  - smart folders
  - counts
- Navigation state chooses the active scope; it does not own business logic.

## Rename Map

- keep `Sidebar`
- remove wording that treats sidebar tree rows as authoritative domain state

## Delete List

- Delete sidebar-specific refresh events.
- Delete renderer-side tree mutation logic that duplicates backend ordering or naming rules.

## DTOs and Commands Involved

- sidebar tree DTOs
- `get_sidebar_tree`
- navigation store types for system, folder, smart-folder, and collection scopes

## Workflows

- Click system node -> navigation selects scope -> grid query updates.
- Drag entity to trash or folder -> backend mutation -> receipt updates sidebar counts and affected scopes.
- Rename folder or smart folder -> backend updates -> published tree refreshes.

## Acceptance Criteria

- Sidebar tree can be regenerated entirely from backend state.
- Navigation store contains selection, not business semantics.
- No sidebar component performs global invalidation.
