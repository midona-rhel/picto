# Browsing and Navigation

[← User Guide](README.md)

## Views

| View | Shows | Shortcut |
|------|-------|----------|
| All | Every active item | `Mod+1` |
| Inbox | Items waiting for review ([Importing](importing-files.md#reviewing-the-inbox)) | `Mod+2` |
| Recently Viewed | Items you opened, newest first. Right-click → **Clear Recently Viewed** to empty it. | — |
| Uncategorized | Active items that are in no folder | — |
| Untagged | Active items with no tags | `Mod+3` |
| Tags | The tag manager ([Tags](tags.md)) | — |
| Random | All active items in a new random order each time you open it | — |
| Subscriptions | [Subscriptions](subscriptions.md) | `Mod+Shift+S` |
| Duplicates | [Duplicate review](duplicates.md) | — |
| Trash | Trashed items. Right-click → **Empty Trash** or **Restore All**. | `Mod+4` |

Folders and smart folders are views too. Use **Back** and **Forward** (`Alt+Left` / `Alt+Right`)
to return to views you visited.

### Quick Access

Right-click a folder or smart folder → **Add to Quick Access** to pin it in the **Quick Access**
section at the top of the sidebar. **Remove from Quick Access** unpins it.

## Layout and sorting

Click **View options** in the title bar (or right-click empty grid space).

**Layout**: Waterfall, Grid, or Justified.

**Sort by**: Date Added, Date Created, Name, File Size, Rating, or Random, with an ascending /
descending toggle (not shown for Random). Folders and Recently Viewed keep their own order and do not
show sort options; right-click a folder → **Sort by** to reorder its contents instead.

**Display** toggles: Show Name, Show Resolution, Show Extension, Show File Type, Show Item Count,
Compact, Fit Thumbnails (Grid layout only), Show Subfolder Content (folders only), Show Sidebar,
Show Inspector.

These choices are saved per view. Defaults for every view, and **Reset all views**, are in
**Settings → Controls → Grid Defaults**.

The slider in the title bar sets the tile size. Settings can make the mouse wheel change the tile
size instead of scrolling.

## Search

Type in **Search** (`Mod+F`) to search the current view. It matches names, notes, source URLs, and
source text, and needs at least three characters.

## Filters

Click **Filter library** to open the filter bar. Each field opens its own picker:

| Filter | Options |
|--------|---------|
| Color | A color and a sensitivity |
| Tags | Tags to include or exclude; Match any, Match all, or Match exactly |
| Folders | Folders to include or exclude; Match any, Match all, or Match exactly |
| Rating | One or more of 5…1 stars and Unrated |
| Type | Media families (Images, Videos, Audio, …) and individual formats present in the view. Right-click a type to exclude it. |
| Date Imported / Date Modified | Today, Yesterday, Last 7 / 30 / 90 / 365 Days, or a custom range |
| Resolution | Minimum and maximum width and height |
| Duration | Minimum and maximum, in seconds, minutes, or hours |
| File Size | Minimum and maximum, in KB or MB |

The bar shows Color, Tags, Folders, Rating, and Type by default. Click **Add filter** (+) to add or
remove fields.

To clear one filter, click its **×** or right-click it. **Clear filters** clears them all.

**Saved filters** (disk icon): type a name and click **Save** to store the current filters. Click a
saved filter to apply it; right-click it to delete it.

**Lock filters** (lock icon): keeps the current filters when you move to another view.

## Selection

| Action | How |
|--------|-----|
| Select | Click |
| Add or remove from selection | `Mod`+click |
| Select a range | `Shift`+click |
| Marquee select | Drag on empty grid space |
| Select all | `Mod+A` |
| Clear selection | `Escape` |

## Keyboard navigation

| Key | Action |
|-----|--------|
| `Arrow` keys or `W` `A` `S` `D` | Move the selection |
| `Home` / `End` | First / last item |
| `Page Up` / `Page Down` | One screen up / down |
| `Shift` + any of the above | Extend the selection |
| `Enter` | Open in the media view |
| `Space` | Quick Look (Settings can make Space scroll the page instead) |

## Context menu

Right-click an item for its actions. The menu adapts to the selection and view. It includes:

- **Accept** / **Reject** (Inbox)
- Open with Default App, Open With Other…, Reveal in Finder / Show in Explorer, Open in New Window
- View options and **Grayscale** (`Mod+Alt+G`, grays out the grid)
- Add to Folder (`Mod+Shift+J`), New Folder with Selection, **Group...**, Edit Group, Ungroup...
  (see [Collections](collections.md))
- Rename / Batch Rename (`Ctrl+R`)
- Copy, Copy File Path (`Mod+Alt+C`), Copy Name, Copy as Link
- Add Tags (`T`), Auto Tag (`Mod+Shift+A`)
- Set Rating
- **Export** → Export Originals..., Export As..., Export as Picto Pack... ([Exporting](exporting.md))
- **Search by Image** (images only): TinEye, SauceNAO, Yandex Images, Sogou, Bing Visual Search
- **More**: Set as Library Cover, Set as Folder Cover (in a folder), Find Items with This Media,
  Regenerate Thumbnail
- Remove from Folder (in a folder)
- Move to Trash (`Mod+Backspace`); in Trash: Restore and Delete Permanently

The full reference is in **Settings → Shortcuts**.
