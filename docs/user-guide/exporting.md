# Exporting

[← User Guide](README.md)

Picto can export copies of your files, converted images, or a Picto Pack that another Picto library
can import. Exporting never changes the library.

Export commands are in the **File** menu, under **Export** in the item context menu, and in the
right-click menu of folders and smart folders (which export everything in them). Exporting a
collection exports all of its members.

## Export originals

**Export Originals…** (`Mod+E`) asks for a destination folder and copies the original files there
unchanged.

## Export As

**Export As…** (`Mod+Shift+E`) opens a dialog:

| Field | Options |
|-------|---------|
| Destination | Click **Browse** to choose a folder. |
| Format | Original Format, JPEG, PNG, WebP, AVIF |
| Quality | 1–100 % (default 90). JPEG, WebP, and AVIF only. |
| Resize (optional) | Width and/or height in pixels |
| Keep Aspect Ratio | On by default |

Only images can be converted. Other media is skipped when a conversion format is chosen; choose
**Original Format** to export it.

## File names

Exported files are named after the item, with the extension of the output format. If a file with
that name already exists, Picto adds a number, for example `photo (2).jpg`.

## Picto Packs

A Picto Pack (`.picto-pack`) is a portable package for moving items between libraries.

It contains original media, collections, names, notes, dates, ratings, tags, source links, and the
definitions of included folders. It never contains subscriptions, provider history, login data, or
watched-folder paths.

### Export a Picto Pack

1. Select items, or right-click a folder or smart folder.
2. Choose **Export as Picto Pack...** (or **File → Export as Picto Pack…**).
3. Click **Choose Destination...** and save the file.

Exporting a smart folder exports only its current matching items, not the smart folder or its rules.

### Import a Picto Pack

Use **File → Import Picto Pack…**, drop a single `.picto-pack` file on the grid, or open the file
with Picto. The dialog shows how many items, media files, folders, and smart folders it holds and
its original size. Click **Import**.
