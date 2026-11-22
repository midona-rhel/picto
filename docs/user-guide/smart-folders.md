# Smart Folders

[← User Guide](README.md)

A smart folder shows every item that matches its rules. You never add items to it by hand. Its
content changes as your library changes.

## Create a smart folder

1. Press `Mod+Shift+Alt+N`, or click **+** next to **Smart Folders** and choose **New Smart
   Folder**.
2. Enter a name. You can also pick an icon and a color and enter notes.
3. Set the rules, which are described below, and click **Create**.

## Rules

The rules are in groups. Each group starts with this line:

> Match **all / any** of the following, **included / excluded**

- With **all**, an item must match every rule of the group.
- With **any**, an item must match at least one rule of the group.
- With **excluded**, the items that match the group are taken out of the results.

When a smart folder has several groups, an item must satisfy every group. Use **+** and **−** to
add or remove rules and groups. A smart folder can have at most **10 rules** in all.

| Field | Operators | Value |
|---|---|---|
| Tags | include all, include any, do not include | One or more tags. |
| File Type | is, is not | Image, Video, Audio, PNG, JPEG or MP4. |
| Rating | equals, not equals, greater than, at least, less than, at most, between | Unrated to 5 stars. |
| File Size | The operators of Rating. | A number in Bytes, Kilobytes, Megabytes or Gigabytes. |
| Date Imported, Date Created, Date Modified | is, after, on or after, before, on or before, between | A date. |
| Name | contains | Text. |
| Width, Height | The operators of Rating. | A number of pixels. |
| Duration | The operators of Rating. | A number of seconds. |
| Notes, Source URL | contains, is empty, is not empty | Text. |
| Color | contains | A color. The rule matches items whose main color is similar to it. |

While you edit the rules of a smart folder that exists, the grid behind the dialog shows the
results of the rules as you change them. **Cancel** puts the previous rules back.

## Nest smart folders

Right-click a smart folder and choose **New Child Smart Folder**. A child searches only in the
results of its parent, so its rules narrow the parent's results. For example:

- The parent has the rule "Tags include all `series:landscape`".
- The child has the rule "Tags include all `wallpaper`".
- The child shows the landscapes that are also wallpapers.

Smart folders can be nested up to 8 levels deep.

A **smart folder group** has no rules of its own. It holds smart folders in the sidebar. To create
one, click **+** next to **Smart Folders** and choose **New Smart Folder Group**, or right-click a
smart folder and choose **New Child Smart Folder Group**.

## Manage smart folders

Right-click a smart folder. The menu has these rows:

- **Edit Smart Folder...**, which changes the name, the icon, the color and the notes, and **Edit
  Rules...**.
- **New Child Smart Folder** and **New Child Smart Folder Group**.
- **Add to Quick Access**, **Rename**, **Move to...** and **Duplicate**.
- **Refresh Results** and **Sort Results by Name**.
- **Expand Smart Folder** or **Collapse Smart Folder**, **Change Icon** and a row of colors.
- **Export**, which is described in [Exporting](exporting.md).
- **Delete**, which removes only the smart folder. The media is not changed.

Creating, editing and deleting a smart folder can be undone with `Mod+Z`.
