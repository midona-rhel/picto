# Getting Started

[← User Guide](README.md)

## Installation

Download the latest release for your platform from the [releases page](https://github.com/midona-rhel/picto/releases):

- **Windows** — Run the `.exe` installer (NSIS) or extract the `.zip`
- **macOS** — Open the `.dmg` and drag Picto to Applications
- **Linux** — Run the `.AppImage` directly, or install the `.deb` package

Picto bundles FFmpeg and gallery-dl automatically — no extra dependencies needed.

## First Launch

When you launch Picto for the first time, you'll need a **library** — a folder where Picto stores your database, thumbnails, and settings.

1. Click **Create New Library**
2. Choose a name and a location on disk
3. Picto creates the library folder and opens it

You can also open an existing library if you've used Picto before, or if someone shared a library folder with you.

See [Library Management](library-management.md) for details on switching between libraries.

## Your First Import

With your library open, import some files:

- **Drag and drop** — Drag image or video files directly onto the grid area
- **File picker** — Press `Ctrl+I` (`Cmd+I` on Mac) to open the file picker
- **Folder import** — Import an entire folder, optionally preserving its structure

Imported files are copied into the library's content-addressed blob store. The originals on disk are not modified.

By default, new imports go to the **Inbox** — a staging area where you can review them before organizing. You can change this in [Settings](settings.md) to import directly as active.

## Orientation

The Picto interface has four main areas:

- **Sidebar** (left) — Navigate between views, folders, smart folders, and system scopes
- **Grid** (center) — Browse your images as thumbnails with multiple layout modes
- **Inspector** (right) — View and edit metadata, tags, ratings, and notes for selected files
- **Toolbar** (top) — Navigation controls, search, view options, sorting, and filtering

Toggle the sidebar with `Ctrl+Alt+1`, the inspector with `Ctrl+Alt+2`, or both with `Tab`.

See [Interface Overview](interface-overview.md) for a detailed walkthrough.

## Next Steps

- [Import more files](importing-files.md) or set up [watched folders](folders.md#watched-folders)
- [Organize with tags](tags.md) and [folders](folders.md)
- Set up [subscriptions](subscriptions.md) for automated downloads
- Explore [keyboard shortcuts](keyboard-shortcuts.md) to work faster
