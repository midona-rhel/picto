# PBI-607: Persist tag groups and favorites

## Problem

Picto currently exposes database namespaces as if they were user-managed groups. A namespace has
only a name and count; there is no canonical record for a group, its color, its display order, or
favorite tags. Renderer-only controls would therefore lose state and misrepresent the product.

## Contract

- The UI calls user-facing collections **Groups**; namespace remains an internal tag qualifier.
- Built-in namespace groups use Picto-owned predefined icons and colors.
- User groups persist name, color, order, and tag membership in SQLite.
- Tags can be favorited independently of group membership and appear in a Favorites view.
- Tag and group mutations follow the canonical IPC, SQLite transaction, projection settlement, and
  resource invalidation path.
- Tag icons are product-defined bookmark glyphs; individual tags do not store arbitrary icons.

## Implementation

1. Add canonical group, membership, ordering, and favorite records to the fresh-library schema.
2. Add controller operations and queries for create, rename, recolor, reorder, delete, membership,
   and favorite mutations.
3. Extend Tags with All, Favorites, built-in namespace groups, and user groups without duplicating
   tag-query logic.
4. Reuse the shared context-menu primitive for tag and group actions.
5. Invalidate tag summaries and affected group resources after each mutation.

## Verification

- Groups and favorites survive restart and preserve order, color, membership, and counts.
- A tag can belong to multiple user groups without changing its namespace.
- Deleting a group does not delete its tags or media assignments.
- Built-in group icons/colors remain deterministic across themes and platforms.
- Context menus expose only actions supported by the selected built-in or user group.

Delete this PBI when the acceptance checks pass. Git history is the archive.
