# Importing Files

[← User Guide](README.md)

Importing copies a file into the library. The original file on disk is not changed and not moved.
A file whose content is already in the library is not stored a second time.
[Supported Formats](supported-formats.md) lists what Picto can import.

## Ways to import

| Method | How |
|--------|-----|
| Files | Choose **File → Import Files…** (`Mod+I`), or right-click empty space in the grid and choose **Import Files...**. |
| Folder | Choose **File → Import Folder…** (`Mod+Shift+I`), right-click empty space in the grid and choose **Import Folder...**, or right-click a folder and choose **Import Folder Here...**. |
| Drag and drop | Drag files, a folder, or images from a browser onto the grid. A drop that carries only a link and no image is not imported, and Picto says so. |
| Paste | Copy files or an image, then press `Mod+V` in the grid, or right-click empty space in the grid and choose **Paste Import**. |
| Picto Pack | Choose **File → Import Picto Pack...**, or drop one `.picto-pack` file on the grid. See [Exporting](exporting.md#picto-packs). |

The system's file dialog shows files of every type, including types that Picto does not import.

When a folder is open in the grid, the imported files are also added to that folder. The menu and
the keys cannot import into the Trash, a smart folder or Recently Viewed.

### Folder import options

Importing a folder opens the **Import Folder** dialog:

| Option | Default | Meaning |
|--------|---------|---------|
| Preserve folder structure | On | Picto makes a Picto folder for each folder on disk. |
| Include subfolders | On | Picto also imports the files in subfolders. |
| Include folders without media | Off | Picto also makes Picto folders for subfolders that hold no supported media. |
| Watch this folder | Off | Picto keeps importing files that are added to the folder later (see [Watched folders](#watched-folders)). This option turns on **Preserve folder structure**. |

### Several files at once

When you import more than one file, Picto asks **Import as Collection?**. **Yes** puts the files
into one [collection](collections.md). **No** imports them as separate items. To stop this
question, set **Settings → Auto-Import → Multiple files** to **Group as collection** or to **Keep
separate**.

## Inbox or library

Every item is in one of three states:

| State | Where the item is shown |
|-------|-------------------------|
| Inbox | In the Inbox view only. |
| Active | In All, in folders, in smart folders and in the other views. |
| Trash | In the Trash view only. |

The state of a new import depends on where it comes from:

- A file that is imported while the **Inbox** view is open goes to the Inbox.
- A file that is imported from any other view becomes active.
- A file from a watched folder gets the status that was chosen for that watched folder, which is
  Inbox by default.
- A file from a [subscription](subscriptions.md) goes to the Inbox.

## Reviewing the Inbox

Open **Inbox** (`Mod+2`) and review the items with these keys:

| Key | Action |
|-----|--------|
| `Z` | Accept. The item becomes active. |
| `X` | Reject. The item goes to the Trash. |

**Settings → Inbox & Deleting → Rejected items** can make Reject delete the item permanently and
not move it to the Trash.

In the grid, `Z` and `X` act on the whole selection. The right-click menu and the **Organize**
menu have **Accept** and **Reject** too. The keys and the menu rows work wherever the selected
items are in the Inbox.

In the media view and in Quick Look, an Accept button (✓) and a Reject button (✗) are shown over
the item. After each decision the viewer goes to the next Inbox item, and it closes when no item
is left.

The number on Picto's icon is the number of items in the Inbox. It changes as items arrive and as
you accept or reject them. The macOS Dock shows it, and so do Linux launchers that show such a
number. Windows does not show it.

Subscriptions wait while the Inbox holds too many items. The limit is 1,000 items by default.
Change it in **Settings → Subscriptions → Inbox Capacity**.

## Watched folders

A watched folder is a folder on disk from which Picto keeps importing.

1. Right-click a Picto folder and choose **Attach Watched Folder...**.
2. In the **Auto-Import Folder** dialog, click **Browse** and pick the folder on disk.
3. Set the options and click **Save**.

| Option | Meaning |
|--------|---------|
| Enable Watch | It turns the watching of this folder on or off. |
| Watch Subfolders | Picto also imports from subfolders. |
| Import Status | New files from this folder go to the **Inbox**, which is the default, or become **Active**. The choice is saved for each watched folder. |

New files are imported into the Picto folder that the watched folder is attached to. To stop,
right-click the folder and choose **Remove Watched Folder**. The switch in **Settings →
Auto-Import** turns all watching on or off.
