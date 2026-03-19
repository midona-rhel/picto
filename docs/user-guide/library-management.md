# Library Management

[← User Guide](README.md)

A library is a self-contained collection — a folder on disk that holds your database, thumbnails, settings, and imported files. You can have multiple libraries and switch between them.

## What's in a Library Folder

```
my-library.library/
├── db/          ← SQLite database (metadata, tags, folders)
├── blobs/       ← Content-addressed file and thumbnail storage
├── plugins/     ← Site plugin configurations
└── settings/    ← Library-specific settings
```

## Creating a Library

1. Open the library manager (File menu or first-launch dialog)
2. Click **Create New Library**
3. Choose a name and location
4. Picto creates the folder structure and opens the library

## Opening an Existing Library

1. Open the library manager
2. Click **Open Library**
3. Navigate to a `.library` folder on disk
4. Picto opens it and adds it to your recent history

## Switching Between Libraries

In the library manager, click any library in your history to switch to it. Picto closes the current library and opens the selected one. Your place in the previous library is preserved for next time.

## Pinning Libraries

Pin frequently-used libraries to keep them at the top of the library list. Click the pin icon next to any library in the manager.

## Renaming a Library

Right-click a library in the manager → **Rename**. This changes the display name without modifying the folder path on disk.

## Relocating a Library

If you move a library folder on disk (e.g., to a different drive), Picto will detect it's missing on next launch and offer to relocate:

1. A dialog appears showing the missing path
2. Click **Relocate** to browse to the new location
3. Picto updates its records

You can also manually relocate from the library manager.

## Removing from History

Right-click a library → **Remove**. This removes it from the recent list without deleting any files. You can re-open it later with the Open Library dialog.

## Deleting a Library

Right-click a library → **Delete**. This permanently deletes the library folder and all its contents (database, thumbnails, and imported files). The currently open library cannot be deleted. **This action is irreversible.**

## Library History

Picto remembers the last 10 libraries you've opened. On launch, it automatically opens the most recently used library.
