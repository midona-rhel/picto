# Folders

[← User Guide](README.md)

Folders group media by hand, like albums. An item can be in several folders at once. Removing an
item from a folder, or deleting a folder, never deletes the media.

## Create folders

| Action | How |
|---|---|
| New folder | `Mod+Shift+N`, or **+** next to **Folders** in the sidebar |
| New subfolder | Select a folder, press `Alt+N`, or right-click it → **New Subfolder** |
| Folder from a selection | Right-click selected items → **New Folder with Selection** |

Folders nest up to **8 levels** deep. When you import or watch a disk folder that is deeper than
the space left, Picto keeps as many levels as fit and puts files from deeper folders into the
nearest kept folder. No files are skipped because of depth.

## Add and remove items

| Action | How |
|---|---|
| Add to folders | Select items, press `F` (or `Mod+Shift+J`), pick folders |
| Add to the last folder you used | Right-click → **Add to "folder name"** |
| Drag | Drag items from the grid onto a folder in the sidebar |
| Remove from the current folder | Inside the folder, press `Mod+Shift+Backspace` or right-click → **Remove from Folder** |

## Manage folders

Right-click a folder in the sidebar:

- **Add to Quick Access**, **Rename**, **Move to…**, **Duplicate**
- **Set Auto Tags…** — see [Auto-tags](#auto-tags)
- **Import Folder Here…**, **Attach Watched Folder…**, **Remove Watched Folder**
- **Sort Folders** — sorts the folder tree A–Z or Z–A, for this level or this level and all
  descendants
- **Sort by** — reorders the items inside the folder by Name, Import Date, Date Created, Date
  Modified, Size or Notes
- **Expand/Collapse Folder**, **Expand/Collapse All Folders**
- **Change Icon** and a color picker
- **Export** and **Delete** (asks for confirmation; deletes the folder and its subfolders, not the
  media)

You can also drag folders in the sidebar to reorder or nest them. Double-clicking a folder renames
it or expands it, depending on **Settings → Sidebar → Double Click**.

Inside a folder, right-click an item → **More → Set as Folder Cover** to choose the folder's cover.

## Auto-tags

A folder can add tags to every item that enters it.

1. Right-click the folder → **Set Auto Tags…**, or select folders and press `Mod+Shift+R`.
2. Add the tags.

From then on, items added to the folder (by hand, by import or by a watched folder) get those tags.
Items already in the folder are not changed.

## Watched folders

A watched folder imports new files that appear in a folder on disk.

1. Right-click a Picto folder → **Attach Watched Folder…**.
2. Click **Browse** and choose the disk folder.
3. Set the options and click **Save**.

| Option | Meaning |
|---|---|
| Enable Watch | Turn off to stop watching |
| Watch Subfolders | Also import from subfolders, recreating them as Picto subfolders |
| Import Status | **Inbox** (default) or **Active**: where new files from this folder land |

Picto checks watched folders about every 30 seconds while the library is open, and waits until a
file has finished copying. A file is imported once; if it later changes on disk (size or
modification time), it is imported again. ZIP files in a watched folder are ignored.

To stop watching, right-click the folder → **Remove Watched Folder**. Media already imported stays.

**Settings → Auto-Import** turns all watched folders on or off at once.
