# PBI-609: Finish remaining folder context operations

## Problem

The main folder interaction pass is implemented. Single folders now support creation, rename,
move, structural duplication, Quick Access, auto tags, tree/content sorting, expand/collapse,
metadata, import/watch, export, and deletion. Multi-selection exposes every applicable implemented
operation rather than collapsing to delete-only behavior.

Completed bulk behavior:

- Add to or remove from Quick Access for mixed folder/smart-folder selections.
- Duplicate every selected folder or smart folder.
- Move homogeneous folder or smart-folder selections.
- Set one auto-tag configuration across selected folders.
- Sort the contents of selected folders or smart folders.
- Expand/collapse selected tree nodes, update color, and delete atomically per domain.

Folder auto tags apply to existing media in the folder hierarchy and to media added later. Removing
an auto-tag rule does not remove tags already written to media. Folder duplication copies structure,
metadata, and auto-tag rules, but intentionally does not copy media memberships or watched paths.

Remaining gaps are deliberately not represented by fake or first-item-only menu actions. Multiple
folder and smart-folder scopes still have no canonical union target, so bulk export cannot yet be
truthful. Copy Link also requires an application deep-link contract before it can be considered a
real operation. Passwords and cross-library export are outside Picto's product model. Showing
subfolder content remains deferred product behavior.

## Contract

- Multi-folder and mixed folder/smart-folder export uses one backend-owned union target with stable
  deduplication, accurate counts, and no renderer materialization of every item.
- Bulk rename is added only with an atomic, collision-safe folder/smart-folder mutation contract.
- Automatic smart-folder invalidation remains the refresh mechanism; no manual Refresh item is
  added merely to imitate another product.
- Sidebar menus never label a first-item-only operation as a bulk action.
- Quick Access is persisted per library in application settings and uses the established star icon.
- A folder link is exposed only after Picto can resolve it through a real deep-link handler.

## Verification

- Bulk export tests cover overlapping scopes, mixed scope kinds, deduplication, and empty results.
- Context-menu tests prove unsupported bulk operations are absent until their handlers exist.
- Folder tests prove structural duplication, auto-tag inheritance, and stable tree sorting.

Delete this PBI when the acceptance checks pass. Git history is the archive.
