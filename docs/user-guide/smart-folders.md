# Smart Folders

[← User Guide](README.md)

A smart folder shows every item that matches its rules. You never add items by hand; the results
change as your library changes.

## Create a smart folder

1. Press `Mod+Shift+Alt+N`, or click **+** next to **Smart Folders** → **New Smart Folder**.
2. Enter a name. Optionally pick an icon, a color and notes.
3. Build the rules (below) and click **Create**.

## Rules

Rules live in groups. Each group reads as a sentence:

> Match **all / any** of the following, **included / excluded**

- **all** — an item must match every rule in the group.
- **any** — one matching rule is enough.
- **excluded** — items matching the group are removed from the results.

When there are several groups, an item must satisfy every group. Use **+** and **−** to add or
remove rules and groups. A smart folder can have at most **10 rules** in total.

| Field | Operators | Value |
|---|---|---|
| Tags | include all, include any, do not include | One or more tags |
| File Type | is, is not | Image, Video, Audio, or a specific accepted format |
| Rating | equals, not equals, greater than, at least, less than, at most, between | Unrated to 5 stars |
| File Size | same as Rating | Number with B, KB, MB or GB |
| Date Imported, Date Created, Date Modified | is, after, on or after, before, on or before, between | Date |
| Name | contains | Text |
| Width, Height | same as Rating | Pixels |
| Duration | same as Rating | Seconds |
| Notes, Source URL | contains, is empty, is not empty | Text |
| Color | contains | A color (matches items with a similar dominant color) |

While you edit the rules of an existing smart folder, the grid behind the dialog updates as you
type. **Cancel** restores the previous rules.

## Nest smart folders

Right-click a smart folder → **New Child Smart Folder**. A child only searches inside its parent's
results, so its rules narrow the parent:

- Parent: Tags include all `series:landscape`
- Child: Tags include all `wallpaper`
- Child shows: landscapes that are also wallpapers

Smart folders nest up to 8 levels.

A **smart folder group** (**+** → **New Smart Folder Group**, or right-click → **New Child Smart
Folder Group**) has no rules of its own. Use it to organize smart folders in the sidebar.

## Manage smart folders

Right-click a smart folder:

- **Edit Smart Folder…** (name, icon, color, notes) and **Edit Rules…**
- **Add to Quick Access**, **Rename**, **Move to…**, **Duplicate**
- **Refresh Results** and **Sort Results by Name**
- **Expand/Collapse**, **Change Icon**, color picker
- **Delete** — removes only the smart folder; media is not affected

Creating, editing and deleting smart folders can be undone with `Mod+Z`.
