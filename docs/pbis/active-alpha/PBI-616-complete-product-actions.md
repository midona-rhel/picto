# PBI-616: Complete product actions and menus

## Goal

Finish Picto's contextual, application, viewer, and library actions with real handlers. Picto's
shared context-menu, controller, undo/history, export, and modal systems remain the implementation
owners.

No menu may contain a disabled placeholder for an unimplemented operation. Each slice adds the
underlying command, failure reporting, platform behavior, focused tests, and a short manual test
script before the next slice begins.

Cross-library copy or move is explicitly excluded. Browser-extension integration is future work and
is not part of this PBI.

## Delivery order

### 1. Grid items and empty grid space

- Open With Other, using the platform application chooser in the item context menu.
- Add to Last Used Folder, persisted only after a successful folder assignment and cleared if stale.
- Collision-safe batch rename with a preview before applying.
- Copy Tags for a real multi-selection, with a defined union/intersection payload rather than the
  first selected item.
- Set as Folder Cover when the current scope is a normal folder.
- Replace the flat export entry with explicit Original, converted-format, and portable-package
  variants.
- Empty-space actions: New Folder, New Smart Folder, Import Files, Import Folder, and Paste Import.
- Empty-space actions reuse existing import/create owners; Paste Import gains a real clipboard-file
  and clipboard-image ingestion route before it is displayed.

### 2. Folders and smart folders

- Copy Folder Link and Copy Smart Folder Link backed by a resolvable Picto deep-link contract.
- Refresh Smart Folder Results through canonical invalidation, including visible loading/error state.
- New Smart Folder Group.
- Batch rename folders and smart folders with collision checks and one history operation.
- Explicit export variants for a single folder or smart folder.
- Bulk export over multiple selected folders/smart folders through a deduplicating union target.
- Distinguish exporting original media from exporting a portable folder package.
- Additional export formats use the shared export pipeline; no surface-specific exporter.

### 3. Tags and tag groups

- Export selected tags and tag groups using a documented portable format.
- Batch rename selected tags.
- Merge an actual multi-selection of tags.
- Treat category/group/namespace as one canonical field: moving a tag rewrites its qualified name,
  and each tag belongs to exactly one group.
- Add one complete tag-group context menu: filter using group, rename, delete, color, and export.
- Add a tag/group list menu with A-Z, Z-A, most-used, and least-used ordering.
- Preserve the existing single-tag edit, merge, Starred, filter, copy, and group movement actions.

### 4. Sidebar utilities

- Full Folder Path display toggle.
- Sidebar section visibility menu.
- Section-level Expand All and Collapse All commands.
- Keep Clear Recently Viewed, Restore All, and Empty Trash under the same shared menu renderer.

### 5. Viewer actions

- One playback context-action owner for video and audio.
- Current-frame thumbnail capture for every playable format that can provide a deterministic frame.
- Actual Size, Fit, and applicable zoom choices without exposing zoom for Flash.
- Reuse the existing video playback semantics and shared context menu; do not create per-format menu
  presentations.

### 6. Application and burger menus

- New Folder, New Smart Folder, and Uncategorized navigation.
- Reload Library and Merge/Import Another Library.
- Sidebar and Inspector visibility toggles.
- Grid layout, sorting, label, badge, and applicable filter controls.
- Recently Viewed and Uncategorized navigation, plus Empty Trash.
- Context-sensitive enabled/visible state based on the focused surface and selection.
- The Picto-styled burger menu and native macOS menu call the same command registry.

### 7. Library management

- Rename Library.
- Reveal Library in Finder/Explorer.
- Relocate/relink a missing library.
- Duplicate Library, Merge/Import Another Library, and Verify/Repair Library.
- Expose the applicable commands from both Library Manager and the compact switcher context menu.
- Preserve the current library icon and pin/history behavior.

## Shared implementation rules

- One command registry owns availability, label, shortcut, and execution for each action. Native
  menus, the Picto burger menu, and contextual menus project that registry rather than reimplementing
  handlers.
- Commands use canonical explicit/query targets and never silently narrow a query-wide selection to
  loaded rows or a multi-selection to its first item.
- Undoable mutations use Picto history. Permanent deletion remains non-undoable.
- Platform-specific actions fail clearly or remain absent where the platform cannot support them.
- Export variants share one export contract and progress/error presentation.
- Existing menu geometry and glass styling remain shared; this PBI changes population and behavior,
  not presentation forks.

## Slice acceptance and manual verification

Every completed slice must include focused automated tests and a user test script that states:

1. which surface and sample data to open;
2. the exact right-click or burger-menu path;
3. which entries should appear or remain absent for single, multi, query-wide, folder, smart-folder,
   collection, viewer, trash, and empty-space states as applicable;
4. the expected mutation, undo/history result, persistence after reload, and failure behavior;
5. the relevant macOS, Windows, and Linux difference.

The PBI is complete only when all entries above have real handlers, focused tests pass, TypeScript
and production builds pass, and no legacy or parallel command path remains.
