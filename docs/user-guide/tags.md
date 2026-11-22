# Tags

[← User Guide](README.md)

Tags describe your media. A tag is written `group:name`, for example `creator:jane_doe` or
`character:alice`. A tag without a group, for example `sunset`, belongs to the group **General**.

## Tag groups

A group (also called a namespace) gives its tags a color and a place in the order of the tags. The
inspector, the tag pickers and the tag manager use both. These groups have a built-in color and
order:

| Group | Use it for |
|---|---|
| `creator` | Artists, photographers and authors. |
| `series` | Franchises, shows and games. |
| `character` | The characters that are shown. |
| `species` | Species and kinds of creatures. |
| `rating` | The content rating. |
| General | Everything else. |

You can create more groups in the tag manager. The groups you create come after the built-in
groups.

To show tags without the name of their group, turn on **Settings → General → Hide group prefixes**.
The tag's group does not change.

## Tag media

1. Select one or more items in the grid.
2. Press `T`, or right-click the selection and choose **Add Tags**.
3. Search for a tag. Click a tag, or press `Enter` on it, to check or uncheck it. `Tab` goes
   through the lists **Recent**, **Selected**, **All** and the tag groups.
4. Click **Apply**. The button shows the number of changes in brackets.

With several items selected, a checked tag is added to every item. Unchecking a tag that the
selection already has removes it from every item.

There are other ways to tag:

- In the **inspector** you can add tags to the selected item and remove them. Type a name that
  does not exist yet and choose **Create "…"** to make a new tag.
- With **Copy Tags** and **Paste Tags** you copy the tags of one item to others. Right-click an
  item and choose **Copy Tags**, or **Copy Shared Tags** when several items are selected. Then
  right-click the items that should get the tags and choose **Paste Tags**.
- A folder can add tags to every item that enters it. See [Folders](folders.md#auto-tags).
- AI models can suggest tags. See [AI Tagging](ai-tagging.md).

## Tag manager

Click **Tags** in the sidebar. The rail on the left shows **All**, **General**, **Starred** and
your **Groups**. Use the search box (`Mod+F`) to find a tag.

Right-click a tag for these actions:

| Action | What it does |
|---|---|
| Filter Items with This Tag | It opens the **All** view with this tag as the only filter. |
| Add to Starred, Remove from Starred | A starred tag is listed under **Starred** here and in the tag pickers. |
| Move to Group…, Add to Group…, Remove from this Group | These change the tag's group. |
| Copy Tag | It copies the tag's name. |
| Edit Tag | It opens a dialog in which you rename, merge or delete the tag. |
| Merge Into… | It moves every use of this tag to another tag that exists, and then removes this tag. |
| Delete Tag | It removes the tag from the library. The media stays. |

Renaming a tag changes it on every item that has it. A rename, a merge and a delete can be undone
with `Mod+Z`.

### Manage groups

- To create a group, click **+** next to **Groups** and enter a name.
- To rename a group, to change its color or to delete it, right-click the group. Deleting a group
  moves its tags to General. No tag is deleted, and no item loses a tag.

### Delete unused tags

**Delete Unused** at the bottom of the rail removes the tags that no item has. A tag that only
items in the Inbox or in the Trash have is kept.
