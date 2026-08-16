# Importing Files

[← User Guide](README.md)

Picto copies files into its content-addressed blob store on import. Your original files on disk are never modified or moved.

## File Picker

Press `Ctrl+I` (`Cmd+I` on Mac) to open the file picker. Select one or more files to import. Picto supports a wide range of [file formats](supported-formats.md).

## Folder Import

Import an entire folder at once. The folder structure can optionally be preserved as Picto folders. This is useful for migrating an existing library.

## Drag and Drop

Drag files or folders from your file manager directly onto the Picto grid. A visual overlay appears to confirm the drop target. Folders are imported recursively.

## Import Status

By default, imported files go to the **Inbox** with status 0. This lets you review new arrivals before organizing them. You can change the default import status in [Settings](settings.md) to import directly as **Active** (status 1).

The three statuses are:
- **Inbox** (0) — Staging area for review
- **Active** (1) — Part of your accepted library
- **Trash** (2) — Marked for deletion

## Watched Folders

You can configure a filesystem folder to be **watched** — Picto will automatically import new files that appear in it. See [Folders — Watched Folders](folders.md#watched-folders) for setup details.

## Subscription Downloads

For automated downloading from websites, see [Subscriptions](subscriptions.md). Subscription imports default to the Inbox.

## What Happens on Import

When a file is imported:

1. Its content hash (SHA-256) is computed
2. The file is stored in the blob store (`blobs/f/<hash>`)
3. A WebP thumbnail is generated
4. Metadata is extracted (dimensions, duration, MIME type)
5. Dominant colors are extracted
6. A perceptual hash is computed for duplicate detection
7. The file appears in the grid

Duplicate files (same content hash) are detected automatically and not imported twice.
