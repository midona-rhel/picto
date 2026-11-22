# Browsing and Navigation

[← User Guide](README.md)

## Views

A view is a list of items that the grid shows. The sidebar lists these views.

| View | What it shows | Shortcut |
|------|---------------|----------|
| All | Every active item. | `Mod+1` |
| Inbox | The items that wait for review. See [Importing](importing-files.md#reviewing-the-inbox). | `Mod+2` |
| Recently Viewed | The items you opened, with the newest first. Right-click the view and choose **Clear Recently Viewed** to empty it. | None |
| Uncategorized | The active items that are in no folder. | None |
| Untagged | The active items that have no tags. | `Mod+3` |
| Tags | The tag manager. See [Tags](tags.md). | None |
| Random | All active items, in a new random order each time you open the view. | None |
| Subscriptions | The Subscriptions screen. See [Subscriptions](subscriptions.md). | `Mod+Shift+S` |
| Duplicates | The Duplicates screen. See [Duplicates](duplicates.md). | None |
| Trash | The items in the Trash. Right-click the view and choose **Empty Trash** or **Restore All**. | `Mod+4` |

Folders and smart folders are views too. The **Back** and **Forward** buttons (`Alt+Left` and
`Alt+Right`) return to views you visited.

### Quick Access

Right-click a folder or a smart folder and choose **Add to Quick Access** to pin it in the
**Quick Access** section at the top of the sidebar. **Remove from Quick Access** unpins it.

## Layout and sorting

Click **View options** in the title bar, or right-click empty space in the grid.

**Layout.** The layouts are Waterfall, Grid and Justified.

**Sort by.** The sort orders are Date Added, Date Created, Name, File Size, Rating and Random. A
switch beside the sort order chooses ascending or descending, except for Random. Folders and
Recently Viewed keep their own order and show no sort options. To put the items of a folder in
another order, right-click the folder and choose **Sort by**.

**Display.** The switches are Show Name, Show Resolution, Show Extension, Show File Type, Show
Item Count, Compact, Fit Thumbnails, Show Sidebar and Show Inspector. Fit Thumbnails works in the
Grid layout only. A folder also has Show Subfolder Content.

Picto saves these choices for each view. The defaults for every view, and the **Reset all views**
button, are in **Settings → Controls → Grid Defaults**.

**Thumbnail size.** The control in the title bar has a **−** button, a **+** button and a knob
between them. The keys `+`, `=` and `-` do the same in the grid.

- In the Waterfall and Grid layouts, one step adds or removes exactly one column. The fewest is
  one column, and the most is the number of columns that tiles 150 px wide give. The knob has one
  position for each number of columns.
- In the Justified layout, the control sets the height of a row, from 150 to 900 px. Dragging the
  knob sets any height in that range. One step multiplies or divides the height by 1.1.
- When the size changes, the tiles move from their old places and sizes to the new ones in
  150 ms. The tile under the pointer stays at the same height on the screen. When the pointer is
  not over the grid, the tile nearest the center of the grid stays at the same height. The check
  box **Animate zoom in the grid** in **Settings → Controls → Grid Defaults** turns the movement
  off.
- A window of the same width opens with the same number of columns after a restart. A wider
  window opens with more columns.

When **Settings → Controls → Mouse wheel** is set to **Adjust thumbnail size**, the mouse wheel
changes the thumbnail size and does not scroll the grid. One notch of a mouse wheel is one step,
and a trackpad makes one step for every 60 px that it scrolls.

## Search

Type in the **Search** field (`Mod+F`) to search the current view. The search looks at names,
notes, source URLs and source text. It needs at least three characters.

### Quick Search

Quick Search is a panel in the middle of the main window with a search field and a list. It finds
commands, views, folders, tags and items by name. Open it in one of these ways:

- Click the **Quick Search** button in the title bar, at the right of the Toggle Panels button.
- Press `Mod+T` or `Mod+K`, or choose **View → Quick Search…**.
- Press `Mod+J`. The panel then lists views, folders, smart folders and subscriptions only.

These keys do nothing while the keyboard is in a text field, a menu or a dialog. They work while
the media view is open. You can change them in **Settings → Shortcuts**.

The list has four groups:

| Group | Rows | What choosing a row does |
|-------|------|--------------------------|
| Actions | At most five commands that can run at this moment, each with its keys. | It runs the command. |
| Places | At most five views, folders, smart folders and subscriptions. | It opens the place. A subscription opens on the Subscriptions screen. |
| Tags | At most five tags, each with the number of its items. The tags on the most items come first. | It opens the **All** view with that tag as the only filter. |
| Items | At most three files or collections, each with its thumbnail, type, dimensions and size. | It opens a view that lists the items with that item's media, and selects the item. A collection then opens in the viewer. |

A name matches when it contains the letters you typed in the same order. Names that start with
your text come first. Names that contain your text as one piece come next. Names that contain its
letters with other letters between them come last. Items are searched only when you typed three
characters or more.

While the field is empty, the list starts with a group named **Recent**, which holds the rows you
chose most recently. Picto keeps the 30 most recent choices for each library.

`Up` and `Down` move the highlight, `Enter` chooses the highlighted row, and `Escape` closes the
panel. The key that opened the panel also closes it. A click on a row chooses it.

## Filters

Click **Filter library** to open the filter bar. Each field of the bar opens its own picker.

| Filter | Options |
|--------|---------|
| Color | A color and a sensitivity. |
| Tags | The tags to include or exclude, and one of Match any, Match all and Match exactly. |
| Folders | The folders to include or exclude, and one of Match any, Match all and Match exactly. |
| Rating | One or more of 5 stars to 1 star and Unrated. |
| Type | Media families such as Images, Videos and Audio, and the single formats that the view contains. Right-click a type to exclude it. |
| Date Imported, Date Modified | Today, Yesterday, the last 7, 30, 90 or 365 days, or a range of dates that you choose. |
| Resolution | The smallest and the largest width and height. |
| Duration | The shortest and the longest duration, in seconds, minutes or hours. |
| File Size | The smallest and the largest size, in KB or MB. |

The bar shows Color, Tags, Folders, Rating and Type by default. Click **Add filter** (**+**) to
add or remove fields.

To clear one filter, click its **×** or right-click it. **Clear filters** clears all of them.

**Saved filters** (the disk icon) stores the current filters under a name: type a name and click
**Save**. Click a saved filter to apply it. Right-click a saved filter to delete it.

**Lock filters** (the lock icon) keeps the current filters when you go to another view.

## Selection

| Action | How |
|--------|-----|
| Select one item | Click it. |
| Add an item to the selection or remove it | `Mod`+click it. |
| Select a range | `Shift`+click the last item of the range. |
| Select with a rectangle | Drag on empty space in the grid. |
| Select all | Press `Mod+A`. |
| Clear the selection | Press `Escape`. |

## Keyboard navigation

| Key | Action |
|-----|--------|
| The arrow keys, or `W` `A` `S` `D` | They move the selection. |
| `Home` and `End` | They go to the first and the last item of the whole view. |
| `Page Up` and `Page Down` | They go one screen up and one screen down. |
| `Shift` with any of the keys above | It extends the selection. |
| `Enter` | It opens the selected item in the media view. |
| `Space` | It opens Quick Look. **Settings → Controls → Space key** can make it scroll the page. |

`Enter`, `Space`, `Mod+C` and `Mod+Backspace` do nothing in the grid while no item is selected.

## Context menu

Right-click an item for its actions. The rows depend on the selection and on the view. The menu
has these rows:

- **Accept** and **Reject**, in the Inbox.
- **Open with Default App**, **Open With Other**, the row that shows the file in the system's
  file manager, and **Open in New Window**. On macOS, **Open With Other** lists the applications
  that can open the file. On Windows, **Open With Other...** opens the system's chooser. On Linux
  the menu has no such row.
- The layout and display options, and **Grayscale** (`Mod+Alt+G`), which shows the grid in gray.
- **Add to Folder** (`Mod+Shift+J`), **New Folder with Selection**, **Group...**, **Edit Group**
  and **Ungroup...** (see [Collections](collections.md)).
- **Rename** for one item, or **Batch Rename** for several (`Ctrl+R`).
- **Copy**, **Copy File Path** (`Mod+Alt+C`), **Copy Name** and **Copy as Link**.
- **Add Tags** (`T`), **Auto Tag** (`Mod+Shift+A`), **Copy Tags** and **Paste Tags**.
- **Set Rating**.
- **Export**, with **Export Originals...**, **Export As...** and **Export as Picto Pack...** (see
  [Exporting](exporting.md)).
- **Search by Image**, for a single image, with TinEye, SauceNAO, Yandex Images and Bing Visual
  Search. Picto sends the picture to the engine you choose and opens the engine's results page in
  your browser. A file that is larger than 1 MiB, or that is not a JPEG, PNG, GIF or WebP file, is
  sent as a JPEG whose long edge is at most 1,600 pixels.
- **More**, with **Set as Library Cover**, **Set as Folder Cover** (in a folder), **Find Items
  with This Media** and **Regenerate Thumbnail**.
- **Remove from Folder**, in a folder.
- **Move to Trash**. In the Trash, **Restore** and **Delete Permanently** take its place.

`Mod+Backspace` moves the selected items to the Trash. In a folder it removes them from the folder
and does not move them to the Trash. In the Trash it deletes them permanently, after a question.
`Mod+Shift+Backspace` restores the selected items in the Trash.

When Copy, Regenerate Thumbnail, Set as Thumbnail or an export fails, Picto shows a notification
that says so.

The full list of keys is in **Settings → Shortcuts**.
