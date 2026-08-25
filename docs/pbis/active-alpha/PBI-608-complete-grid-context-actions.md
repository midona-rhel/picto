# PBI-608: Complete grid context actions

## Problem

Picto's grid menu supports opening with the default application and renaming one item, but it has
no product path for choosing another installed application, renaming a selection as a batch, or
adding a selection directly to the last-used folder. Presenting disabled or no-op menu entries is
not acceptable; these actions remain absent until their handlers are real.

## Contract

- **Open With Other** resolves installed applications through a platform-owned API and opens one
  selected media file with the chosen application on macOS, Windows, and Linux where supported.
- **Batch Rename** previews the resulting names, rejects collisions and invalid filenames, and
  applies one canonical mutation to an explicit selection.
- **Add to Last Used Folder** appears only after a successful folder assignment and targets the
  persisted canonical folder ID; a missing/deleted folder removes the shortcut.
- Collections and query-wide selections expose only actions whose semantics are explicitly
  supported; they never silently collapse to a cover file or loaded page.
- Ordered collection members have an explicit media-member target, separate from root-only
  `ItemTarget`. It supports Add/Remove/Paste Tags, Rating, metadata rename and export for one or
  many member IDs without widening the operation to the collection root.
- Media-member mutations invalidate and return the owning collection root while preserving member
  order, cover and membership. A member removed during an operation is rejected atomically.
- Every visible menu item has a working handler and reports actionable failures.

## Verification

- Platform tests cover installed-application discovery and the unsupported-platform state.
- Batch rename tests cover preview, collision rejection, partial failure rollback, and invalidation.
- Last-used-folder tests cover persistence, deletion, and explicit/query-wide selection targets.
- Context-menu tests prove unsupported actions are omitted rather than disabled or inert.
- Collection tests prove member tags/rating/rename/export address only the selected media IDs and
  that root-only lifecycle/folder operations cannot accept a member target.

Delete this PBI when the acceptance checks pass. Git history is the archive.
