# Exporting

[← User Guide](README.md)

Picto can export copies of your files, converted images, one ZIP archive of the original files, or
a Picto Pack that another Picto library can import. Exporting never changes the library.

The export commands are in the **File** menu, under **Export** in the right-click menu of an item,
and under **Export** in the right-click menu of a folder or a smart folder. The commands of a
folder or a smart folder export everything in it. Exporting a collection exports all of its
members.

When an export fails, Picto shows a notification that says so.

## Export originals

**Export Originals…** (`Mod+E`) asks for a folder and copies the original files there unchanged.

## Export As

**Export As…** (`Mod+Shift+E`) opens a dialog with these fields:

| Field | Options |
|-------|---------|
| Destination | Click **Browse** to choose a folder. |
| Format | Original Format, JPEG, PNG, WebP, AVIF or ZIP Archive. |
| Quality | From 1 to 100 %, with 90 % as the default. The field is shown for JPEG, WebP and AVIF. |
| Resize (optional) | A width, a height, or both, in pixels. The field is shown for JPEG, PNG, WebP and AVIF. |
| Keep Aspect Ratio | The switch is on by default. |

Only images can be converted. When you choose JPEG, PNG, WebP or AVIF, other media is skipped.
Choose **Original Format** to export other media.

**ZIP Archive** writes the original files into one archive named `Picto Export.zip` in the folder
you chose.

## File names

An exported file gets the name of its item and the extension of the output format. If a file with
that name already exists in the folder, Picto adds a number to the name, for example
`photo (2).jpg`.

## Picto Packs

A Picto Pack is a file with the ending `.picto-pack`. It carries items from one library to
another.

A Picto Pack holds the original media, the collections, names, notes, dates, ratings, tags and
source links, and the definitions of the folders that are included. It never holds subscriptions,
the history of the sources, login data or the paths of watched folders.

### Export a Picto Pack

1. Select items, or right-click a folder or a smart folder.
2. Choose **Export as Picto Pack...**, or **File → Export as Picto Pack...**.
3. Click **Choose Destination...** and save the file.

Exporting a smart folder exports the items that match it at that moment. The pack does not hold
the smart folder or its rules.

### Import a Picto Pack

Choose **File → Import Picto Pack...**, or drop one `.picto-pack` file on the grid. The dialog
shows how many items, media files, folders and smart folders the pack holds, and its original
size. Click **Import**.

Opening a `.picto-pack` file from the system's file manager imports it only while Picto is running
with a library open, and only on macOS. A pack that starts Picto is not imported. On Windows and
Linux, a pack that is opened from the file manager while Picto runs is not imported either. Use
the menu command or drop the file on the grid in those cases.
