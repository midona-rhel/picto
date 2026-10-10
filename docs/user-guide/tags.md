# Tags

[← User Guide](README.md)

Tags describe your media. A tag is written `group:name`, for example `creator:jane_doe` or
`character:alice`. A tag without a group (`sunset`) belongs to **General**.

## Tag groups

Groups (namespaces) sort and color tags in the inspector, tag pickers and the tag manager. These
groups have built-in colors and order:

| Group | Use for |
|---|---|
| `creator` | Artists, photographers, authors |
| `series` | Franchises, shows, games |
| `character` | Characters shown |
| `species` | Species or creature types |
| `rating` | Content rating |
| General | Everything else |

You can create more groups in the tag manager. Custom groups sort after the built-in ones.

To show tags without their group prefix, turn on **Settings → General → Hide group prefixes**. The
group itself does not change.

## Tag media

1. Select one or more items in the grid.
2. Press `T`, or right-click and choose **Add Tags**.
3. Search, then click a tag (or press `Enter` on it) to check or uncheck it. `Tab` switches between
   All, the tag groups, Selected and Recent.
4. Click **Apply**.

With several items selected, checked tags are added to every item, and unchecking a tag that is
already on the selection removes it from every item.

Other ways to tag:

- **Inspector** — add or remove tags for the selected item. Type a name that does not exist yet and
  choose **Create "…"** to make a new tag.
- **Copy Tags / Paste Tags** — right-click an item and choose **Copy Tags** (or **Copy Shared
  Tags** for a multi-selection), then right-click the target items and choose **Paste Tags**.
- **Folder auto-tags** — see [Folders](folders.md#auto-tags).
- **AI suggestions** — see [AI Tagging](ai-tagging.md).

## Tag manager

Click **Tags** in the sidebar. The left rail shows **All**, **General**, **Starred** and your
**Groups**. Use the search box (`Mod+F`) to find a tag.

Right-click a tag for:

| Action | What it does |
|---|---|
| Filter Items with This Tag | Shows the items that have the tag |
| Add to Starred / Remove from Starred | Starred tags appear under **Starred** here and in the tag pickers |
| Move to Group… / Add to Group… / Remove from this Group | Changes the tag's group |
| Copy Tag | Copies the tag name |
| Edit Tag | Opens the editor: rename, merge or delete |
| Merge Into… | Moves every use of this tag to an existing tag, then removes this tag |
| Delete Tag | Removes the tag from the library; the media stays |

Renaming a tag updates every item that uses it. Rename, merge and delete can be undone with
`Mod+Z`.

### Manage groups

- **Create** — click **+** next to **Groups** and enter a name.
- **Rename**, **color** or **delete** — right-click the group. Deleting a group moves its tags to
  General; no tags or assignments are deleted.

### Delete unused tags

**Delete Unused** at the bottom of the rail removes tags that no item uses. Tags used only by Inbox
or Trash items are kept.
