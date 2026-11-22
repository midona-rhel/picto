# Interface Overview

[← User Guide](README.md)

The main window has a title bar, a sidebar, the grid and the inspector.

## Title bar

The title bar holds these controls, from left to right.

| Control | What it does |
|---------|--------------|
| Application menu (Windows and Linux) | This button opens the Library, File, Edit, Organize, View, Window and Help menus. On macOS these menus are in the system menu bar. |
| Settings (gear) | This button opens the Settings window (`Mod+,`). |
| Toggle Panels | This button hides the sidebar and the inspector when at least one of them is showing, and shows both when both are hidden (`Tab`). Right-click it for a menu with **Toggle All Panels**, **Toggle Sidebar** and **Toggle Inspector**. |
| Quick Search | This button opens the Quick Search panel (`Mod+T` or `Mod+K`). See [Browsing](browsing-and-navigation.md#quick-search). |
| Back / Forward | These buttons go through the views you visited (`Alt+Left` and `Alt+Right`, or `Mod+[` and `Mod+]`). |
| View title | The title names the current view. For a folder or a smart folder it shows the full path. |
| Thumbnail size | The **−** and **+** buttons and the knob between them make the tiles smaller or larger. See [Browsing](browsing-and-navigation.md#layout-and-sorting). |
| View options | This button opens a menu with the layout, the sort order and the tile display. See [Browsing](browsing-and-navigation.md#layout-and-sorting). |
| Filter library | This button shows the filter bar. A badge on it shows how many filters are set. |
| Search | This field searches the current view (`Mod+F`). |

When the title bar is narrow, the thumbnail size control is hidden first, and then the search
field.

While an item is open in the media view, the title bar shows the viewer's controls in place of the
grid's controls: a back button, the zoom control with **Fit to window** and **Actual size**, and
the **Previous** and **Next** buttons. While the Tags, Subscriptions or Duplicates screen is open,
the title bar shows that screen's buttons.

## Sidebar

- The **library switcher** is at the top and names the open library. Click it to search your
  libraries, to switch to another library, or to choose **Create Library…**, **Open Library…** or
  **Library Manager…**.
- The **views** are All, Inbox, Recently Viewed, Uncategorized, Untagged, Tags, Random,
  Subscriptions, Duplicates and Trash. See [Browsing](browsing-and-navigation.md#views).
- **Quick Access** lists the folders and smart folders that you pinned.
- **Smart Folders** and **Folders** list your smart folders and folders. See
  [Smart Folders](smart-folders.md) and [Folders](folders.md).
- The **Filter** box shows only the folders and smart folders whose names contain the text you
  type.

Right-click a view to choose which views and sections the sidebar shows. **All** and **Trash** are
always shown. Drag the edge of the sidebar to change its width. `Mod+Alt+1` hides or shows the
sidebar.

## Grid

The grid shows the items of the current view. Click an item to select it. Double-click an item or
press `Enter` to open it. Press `Space` for Quick Look. Right-click an item for its actions. See
[Browsing and Navigation](browsing-and-navigation.md) and
[Viewing Images and Video](viewing-images-and-video.md).

## Inspector

The inspector shows the selected item. When several items are selected, it shows a summary of
them. It has these parts:

- A preview and the item's colors. Click a color to filter the view by it.
- The **Name**, the **Notes** and the source URLs, all of which you can edit.
- The **Folders** and the **Tags** that the item has.
- The **Properties**: the rating, which you set by clicking the stars or by pressing `0` to `5` in
  the grid, and facts about the file.
- The **Details**, which hold further metadata when the item has any.

The pin button at the top of the inspector keeps the inspector on the current item while you
select other items. Drag the edge of the inspector to change its width, which can be from 260 to
550 px. `Mod+Alt+2` hides or shows the inspector.

On macOS and Windows, a switch in **Settings → General** makes the sidebar and the inspector
translucent. See [Settings](settings.md#general).

## Native menus

On macOS the menus are in the system menu bar. On Windows and Linux the Application menu button in
the title bar opens them.

| Menu | Contents |
|------|----------|
| Picto (macOS only) | About Picto, Preferences… (`Mod+,`), Hide Picto, Hide Others, Show All, Quit Picto |
| Library | Library Manager… (`Mod+Shift+L`), New Library… (`Mod+N`), Open Library…, Recent Libraries, Subscriptions… (`Mod+Shift+S`) |
| File | New Folder, New Smart Folder, Import Files…, Import Folder…, Import Picto Pack..., Export Originals…, Export As…, Export as Picto Pack..., Check for Updates…, Close Window (macOS). On Windows and Linux this menu also has Settings… and Quit. |
| Edit | Rename or Batch Rename…, Undo, Redo, Cut, Copy, Paste, Copy File Path, Copy Name, Copy as Link, Regenerate Thumbnail, Select All, Deselect All, Remove from Folder, Move to Trash. In the Trash, Restore and Delete Permanently take the place of Move to Trash. |
| Organize | Accept and Reject (in the Inbox), Add Tags…, Add to Folder…, Auto Tag…, Set Rating, Tags |
| View | Open with Default App, Reveal in Finder (Show in Explorer on Windows, Show in File Manager on Linux), Open in New Window, All Images, Inbox, Untagged, Trash, Quick Search…, Duplicates, Log Window, Toggle Full Screen |
| Window | Minimize, Zoom, and Bring All to Front (macOS) or Close (Windows and Linux) |
| Help | About Picto |

A row is disabled while it has nothing to act on. **Copy** copies the files of the selected items.
**File → New Folder** creates a folder in the sidebar with its name ready to be typed.
**Organize → Tags** opens the tag manager.

On macOS, **File → Close Window** (`Mod+W`) hides the main window and Picto keeps running. On
Windows and Linux, closing the main window quits Picto.

The full list of keys is in **Settings → Shortcuts** (see [Settings](settings.md)).
