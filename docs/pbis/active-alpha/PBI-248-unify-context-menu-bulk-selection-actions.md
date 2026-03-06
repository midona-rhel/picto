# PBI-248: Unify context menu behavior for single and bulk selection actions

Supersedes: ~~PBI-230~~ (batch inbox accept/reject — merged into this PBI)

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. Context menu actions do not consistently handle the distinction between single-item and multi-selection operations.
2. Some actions that should operate on the full selection (e.g. add to folder, accept/reject) only apply to the right-clicked item.
3. Specifically: a user tried Ctrl+A → right-click → Accept in the inbox, but only one item was processed. The workaround is opening each image individually and using Enter/Backspace one at a time.
4. Some actions that are single-item-only (e.g. rename) have no defined behavior when multiple items are selected.

## Problem
The context menu does not have a clear policy for how each action behaves when one vs many items are selected. This leads to:
- Bulk operations that silently apply to only one item (e.g. batch accept/reject in inbox)
- Single-item operations that are ambiguous when shown with a multi-selection (e.g. rename — which item?)
- Inconsistent user expectations about what "right-click with selection" means

## Desired policy

Each context menu action should be classified as one of:

| Type | Behavior with multi-selection | Examples |
|---|---|---|
| **Bulk** | Applies to all selected items | Add to folder, Accept, Reject, Delete, Add tags, Remove tags |
| **Primary-only** | Applies to the right-clicked (primary) item only | Rename, Open in new window, Copy hash, View file info |
| **Selection-dependent** | Shows different UI based on selection count | Create collection (needs 2+), Merge duplicates (needs 2) |

## Scope
- `src/components/image-grid/useGridContextMenu.tsx` (or equivalent) — context menu action definitions
- Each action handler — ensure it passes the correct set of entity IDs (selection vs single item)
- Backend: ensure accept/reject and other bulk mutations handle arrays of entity IDs in a single transaction

## Implementation
1. Define a `ContextMenuActionType` enum or config: `'bulk' | 'primary_only' | 'selection_dependent'`.
2. Tag each context menu action with its type.
3. For **bulk** actions: always pass the full selection's entity IDs to the handler. If only one item is selected (or right-clicked without selection), pass just that one.
4. For **primary-only** actions: always pass only the right-clicked item's ID, regardless of selection. Optionally show "(applies to [filename])" in the menu label to clarify.
5. For **selection-dependent** actions: show/hide or enable/disable based on selection count.
6. Visually indicate selection count in the context menu header (e.g. "3 items selected") so the user knows what bulk actions will affect.
7. Add keyboard shortcuts for common bulk actions: Ctrl+Enter (accept selected), Ctrl+Backspace (reject selected).
8. After bulk operations, update grid state to reflect the change on all affected items and show a brief toast: "Accepted N items", "Deleted N items", etc.

## Acceptance Criteria
1. Bulk actions (add to folder, accept, reject, delete, add/remove tags) apply to all selected items.
2. Primary-only actions (rename, open in new window) apply to the right-clicked item only.
3. Context menu shows selection count when multiple items are selected.
4. No action silently operates on fewer items than the user expects.
5. Ctrl+A in inbox → right-click → Accept — all items accepted.
6. Keyboard shortcuts (Ctrl+Enter, Ctrl+Backspace) work for batch accept/reject.
7. Bulk operations complete without timeout or UI freeze for 500+ items.

## Test Cases
1. Select 5 images, right-click → Delete → all 5 deleted.
2. Select 5 images, right-click → Rename → only the right-clicked image gets renamed.
3. Select 5 images, right-click → Add to folder → all 5 added.
4. Select 1 image, right-click → Delete → that 1 image deleted.
5. Select 0 images (right-click on an unselected image) → actions apply to that single image.
6. Context menu header shows "5 items selected" when appropriate.
7. Ctrl+A in inbox (50 items), right-click → Accept → all 50 accepted, inbox empties.
8. Select 10 items, Ctrl+Enter → 10 accepted, remaining items still in inbox.
9. Select 5 items, right-click → Reject → 5 rejected and removed.
10. Accept 500 items — completes without timeout or UI freeze.

## Risk
Low. Primarily a policy definition + ensuring each handler receives the correct ID set. Backend likely already supports bulk operations — the issue is the frontend passing only one ID.
