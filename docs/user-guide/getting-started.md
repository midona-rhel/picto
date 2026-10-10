# Getting Started

[← User Guide](README.md)

## Install

Download the latest release from the [releases page](https://github.com/midona-rhel/picto/releases).

| Platform | Package |
|----------|---------|
| Windows | `.exe` installer |
| macOS | `.dmg` (drag Picto to Applications) |
| Linux | `.AppImage` (make it executable and run it) |

FFmpeg is bundled. Nothing else needs to be installed.

## Create your first library

A library is a folder on disk (`<name>.library`) that holds Picto's database and a copy of every
imported file.

1. On first launch Picto shows **Open a library to start**. Click **Choose Library…**.
2. In the Library Manager, click **New Library…** and enter a name.
3. Click **Choose Location…** and pick the folder that will contain the library.

Picto creates the library and opens it. On later launches it reopens the last library you used.
See [Library Management](library-management.md) for switching, renaming, and moving libraries.

## Import some files

- Drag files or a folder onto the grid.
- **File → Import Files…** (`Mod+I`) or **File → Import Folder…** (`Mod+Shift+I`).
- Copy files or an image and press `Mod+V` in the grid.

Picto copies files into the library. Your originals are not changed.

Files imported while the **Inbox** view is open go to the Inbox for review. Files imported
anywhere else go straight into the library. See [Importing Files](importing-files.md).

## Find your way around

- **Sidebar** (left): views, Quick Access, smart folders, folders.
- **Grid** (center): thumbnails of the current view.
- **Inspector** (right): name, notes, tags, folders, rating, and properties of the selection.
- **Title bar** (top): back/forward, view options, filters, search.

`Mod+Alt+1` toggles the sidebar, `Mod+Alt+2` the inspector, `Tab` both.
See [Interface Overview](interface-overview.md).

## Next steps

- Organize with [tags](tags.md), [folders](folders.md), and [smart folders](smart-folders.md)
- Group related media into [collections](collections.md)
- Download automatically with [subscriptions](subscriptions.md)
- Learn the keyboard shortcuts in **Settings → Shortcuts**
