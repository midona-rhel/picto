# Importing Files

[← User Guide](README.md)

Importing copies a file into the library. The original on disk is not changed or moved. A file
whose content is already in the library is not stored twice.
See [Supported Formats](supported-formats.md) for what Picto can import.

## Ways to import

| Method | How |
|--------|-----|
| Files | **File → Import Files…** (`Mod+I`), or right-click empty grid space → **Import Files...** |
| Folder | **File → Import Folder…** (`Mod+Shift+I`), right-click empty grid space → **Import Folder...**, or right-click a folder → **Import Folder Here...** |
| Drag and drop | Drag files, a folder, or images from a browser onto the grid. |
| Paste | Copy files or an image, then press `Mod+V` in the grid (or right-click → **Paste Import**). |
| Picto Pack | **File → Import Picto Pack…**, or drop one `.picto-pack` file on the grid. See [Exporting](exporting.md#picto-packs). |

When a folder is open in the grid, imported files are also added to that folder. Paste import is not
available in Trash, smart folders, and Recently Viewed.

### Folder import options

Importing a folder opens **Import Folder**:

| Option | Default | Meaning |
|--------|---------|---------|
| Preserve folder structure | On | Recreates the folder tree as Picto folders. |
| Include subfolders | On | Imports files from subfolders too. |
| Include folders without media | Off | Also creates Picto folders for subfolders that contain no supported media. |
| Watch this folder | Off | Keeps importing new files added to the folder later (see [Watched folders](#watched-folders)). Turns on **Preserve folder structure**. |

### Several files at once

When you import more than one file, Picto asks **Import as Collection?**. **Yes** puts them into one
[collection](collections.md); **No** imports them as separate items. To stop being asked, set
**Settings → Auto-Import → Multiple files** to **Group as collection** or **Keep separate**.

## Inbox or library

Every item is in one of three states:

| State | Shown in |
|-------|----------|
| Inbox | The Inbox view only |
| Active | All, folders, smart folders, and the other views |
| Trash | The Trash view only |

Where a new import lands:

- Imported while the **Inbox** view is open: Inbox.
- Imported from any other view: Active.
- From a watched folder: the status chosen for that watch (Inbox by default).
- From [subscriptions](subscriptions.md): Inbox.

## Reviewing the Inbox

Open **Inbox** (`Mod+2`) and review items with these keys:

| Key | Action |
|-----|--------|
| `Z` | Accept: move the item into the library (Active). |
| `X` | Reject: move the item to Trash. |

In the grid, `Z` and `X` act on the whole selection. The right-click menu and the **Organize** menu
have **Accept** and **Reject** too.

In the media view or Quick Look, Accept (✓) and Reject (✗) buttons float over the item. After each
decision the viewer moves to the next Inbox item and closes when none are left.

Subscriptions pause when the Inbox holds too many items (1,000 by default; change it in
**Settings → Subscriptions → Inbox Capacity**).

## Watched folders

A watched folder is a folder on disk that Picto keeps importing from.

1. Right-click a Picto folder → **Attach Watched Folder...**.
2. In **Auto-Import Folder**, click **Browse** and pick the folder on disk.
3. Set the options and click **Save**.

| Option | Meaning |
|--------|---------|
| Enable Watch | Turns the watch on or off. |
| Watch Subfolders | Also imports from subfolders. |
| Import Status | **Inbox** (default) or **Active**: where new files from this folder land. Saved per watched folder. |

New files are imported into the Picto folder the watch is attached to. To stop, right-click the
folder → **Remove Watched Folder**. **Settings → Auto-Import** turns all watching on or off.
