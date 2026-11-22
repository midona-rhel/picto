# Folders

[← User Guide](README.md)

You put media into folders by hand, as into albums. An item can be in several folders at once.
Removing an item from a folder never deletes the media, and neither does deleting a folder.

## Create folders

| Action | How |
|---|---|
| Create a folder | Press `Mod+Shift+N`, or click **+** next to **Folders** in the sidebar. |
| Create a subfolder | Select a folder and press `Alt+N`, or right-click the folder and choose **New Subfolder**. |
| Create a folder from a selection | Right-click the selected items and choose **New Folder with Selection**. |

A new folder appears in the sidebar with its name ready to be typed.

Folders can be nested up to **8 levels** deep. When you import or watch a folder on disk that is
deeper than the levels that are left, Picto keeps as many levels as fit and puts the files of
deeper folders into the nearest folder that it kept. No file is skipped because of the depth.

## Add and remove items

| Action | How |
|---|---|
| Add items to folders | Select the items, press `F` or `Mod+Shift+J`, and pick the folders. |
| Add items to the folder you used last | Right-click the items and choose **Add to "folder name"**. |
| Add items by dragging | Drag the items from the grid onto a folder in the sidebar. |
| Remove items from the open folder | Inside the folder, select the items and press `Mod+Backspace`, or right-click them and choose **Remove from Folder**. |

Inside a folder, `Mod+Backspace` removes the selected items from the folder and does not move them
to the Trash. To move them to the Trash, right-click them and choose **Move to Trash**.

## Manage folders

Right-click a folder in the sidebar. The menu has these rows:

- **New Folder** and **New Subfolder**.
- **Add to Quick Access**, **Rename**, **Move to...** and **Duplicate**.
- **Set Auto Tags...**, which is described under [Auto-tags](#auto-tags).
- **Import Folder Here...**, **Attach Watched Folder...** and **Remove Watched Folder**.
- **Sort Folders**, which sorts the folders in the sidebar from A to Z or from Z to A. It sorts
  either this level, or this level and all levels below it.
- **Sort by**, which puts the items inside the folder in the order of their Name, Import Date,
  Date Created, Date Modified, Size or Notes.
- **Expand Folder** or **Collapse Folder**, and **Expand All Folders** or **Collapse All Folders**.
- **Change Icon** and a row of colors.
- **Export**, which is described in [Exporting](exporting.md).
- **Delete**, which asks first. It deletes the folder and its subfolders, and it does not delete
  the media.

You can also drag folders in the sidebar to change their order or to put one inside another. A
double-click on a folder renames it or expands it. **Settings → Sidebar → Double Click** chooses
which of the two.

To choose the cover of a folder, open the folder, right-click an item and choose **More → Set as
Folder Cover**.

## Auto-tags

A folder can add tags to every item that enters it.

1. Right-click the folder and choose **Set Auto Tags...**, or select folders and press
   `Mod+Shift+R`.
2. Add the tags.

From then on, every item that is added to the folder gets those tags. This holds for items you
add by hand, for imported items and for items from a watched folder. Items that are already in
the folder are not changed.

## Watched folders

A watched folder is a folder on disk whose new files Picto imports.

1. Right-click a Picto folder and choose **Attach Watched Folder...**.
2. Click **Browse** and choose the folder on disk.
3. Set the options and click **Save**.

| Option | Meaning |
|---|---|
| Enable Watch | Turn it off to stop watching this folder. |
| Watch Subfolders | Picto also imports from subfolders, and makes a Picto subfolder for each of them. |
| Import Status | New files from this folder go to the **Inbox**, which is the default, or become **Active**. |

Picto looks at the watched folders about every 30 seconds while the library is open, and it waits
until a file has been copied completely. A file is imported once. If its size or its modification
time changes on disk later, it is imported again. ZIP files in a watched folder are ignored.

To stop watching, right-click the folder and choose **Remove Watched Folder**. The media that was
already imported stays.

The switch in **Settings → Auto-Import** turns all watched folders on or off at once.
