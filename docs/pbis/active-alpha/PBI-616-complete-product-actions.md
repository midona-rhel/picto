# PBI-616: Remaining product actions

This PBI is optional post-release scope. None of its actions block the current release.

## Current state

The release path has real handlers for Open With Other, batch media rename, Add to Last Used
Folder, folder duplication, Quick Access, folder auto tags, folder and smart-folder movement and
sorting, smart-folder refresh, exports, and the active grid/viewer/sidebar context actions.

Do not recreate those actions or keep older parity tickets that claim they are absent.

## Remaining optional actions

- Add resolvable Picto deep links before exposing Copy Folder Link or Copy Smart Folder Link.
- Add one backend-owned deduplicating union target before exporting several folder and smart-folder
  scopes together.
- Add collision-safe batch rename for folder, smart-folder, and tag selections.
- Define a portable tag/group export format before showing tag export actions.
- Add library duplicate, merge/import, relocation, and verify/repair only after each operation has a
  clear failure and recovery contract.
- Project native menus, the Picto menu, and context menus from one command registry where they still
  duplicate availability or shortcut rules.

## Rules

- No disabled placeholders or first-selected-item fallbacks.
- Query-wide and multi-scope operations remain backend-owned and deduplicated.
- Mutations use normal history and compact invalidation; permanent deletion remains non-undoable.
- Platform-specific actions are absent when the platform cannot implement them truthfully.

## Verification

Each added action needs one behavior test through its production owner and one application smoke
covering single, multi-selection, query-wide, and unsupported states where applicable.

Delete this PBI when the remaining actions are either implemented or explicitly dropped from the
product. Git history is the archive.
