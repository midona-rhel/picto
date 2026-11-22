# Settings

[← User Guide](README.md)

Open the Settings window with `Mod+,`, with the gear button in the title bar, with **Picto →
Preferences…** on macOS, or with **File → Settings…** on Windows and Linux. Use the search box at
the top of the window's sidebar to find a setting.

Most changes take effect when you click **Apply** or **Save & Close**. A theme and the switch for
the translucent sidebar show in the main window at once, and closing the Settings window without
saving puts the saved choice back. Cloud actions and downloads of AI models happen at once.

## General

| Setting | Options |
|---|---|
| Theme | Auto, Light, Light Gray, Gray, Dark, Blue or Purple. |
| Zoom Level | 75%, 80%, 90%, 100%, 110%, 125% or 150%. Picto saves this choice, but the choice does not change the size of the interface. |
| Frosted glass sidebar and inspector (macOS), Acrylic sidebar and inspector (Windows) | On or off. It is off by default. |
| Language | English, Deutsch, Español, Português, Français, 简体中文, 日本語 or Suomi. |
| Show item counts in the sidebar | On or off. |
| Hide group prefixes | On or off. When it is on, tags are shown without the name of their group (see [Tags](tags.md)). |

To change the size of the interface, press `Mod+=` to make it larger, `Mod+-` to make it smaller
and `Mod+0` to set it back to 100%. One press changes the size by 5%, between 50% and 200%. The
size applies to every window at once, and Picto keeps it on this computer. The keys do nothing
while the keyboard is in a text field, a menu or a dialog. While an image, a document or a font is
open in the viewer, and on the Duplicates screen, `Mod+0` shows the picture at its actual size and
does not change the size of the interface.

The switch **Frosted glass sidebar and inspector** makes the sidebar and the inspector of the main
window translucent: what is behind the window shows through them, blurred, under the theme's
color. The rest of the window stays opaque, and so do the Settings window, the separate item
windows and the Library Manager. The switch works with every theme.

- On Windows the switch is named **Acrylic sidebar and inspector**. While it is on, the text of
  the main window is drawn with grayscale antialiasing.
- The switch is not shown on Linux. It is not shown on macOS while **Reduce Transparency** is on
  in System Settings.
- A library that is used on macOS and on Windows keeps one choice for each of the two systems.

## Updates

This section shows the installed version and the **Check Now** button. When an update is ready,
it shows **Restart and Install** on Windows and Linux, or **Open Download Page** on macOS. See
[Updates](auto-updates.md).

## Library

This section shows numbers about the open library. You cannot change them here. It shows the
numbers of items (All, Inbox, Trash, standalone items and groups), the media by type, the physical
files, the size, and the numbers of tags, folders, smart folders and subscriptions.

## Sidebar

- **Double Click** chooses what a double-click on a folder does: **Rename** or **Expand/Collapse**.
  **Expand/Collapse** is the default.
- **Show these items in the sidebar** has a check box for each of Uncategorized, Untagged, Inbox,
  Random, Tag Manager, Recently Viewed, Subscriptions, Duplicates, Quick Access, Smart Folders and
  Folders. All and Trash are always shown.
- **Show hierarchy guides** shows or hides the guide lines of the folder tree.

## Controls

| Setting | Options |
|---|---|
| Mouse wheel | Scroll grid, or Adjust thumbnail size. |
| Media view gestures (macOS only) | Wheel zoom, or Trackpad pan + pinch zoom. |
| Double-click | Open Media View, or Open in default app. |
| Middle-click | Open in new window, or Do nothing. |
| Space key | Quick Look, or Scroll page. |

The **Grid Defaults** apply to every view that has no saved choices of its own:

- **Default layout** is Waterfall, Grid or Justified.
- **Thumbnail size** goes from 150 to 600 px in steps of 10 px. The default is 220 px. Each grid
  turns the size into the number of columns that fit its width.
- **Grid spacing** is Wide or Tight.
- **Sort by** is Date Added, Date Created, Name, Rating, File Size or Random, and **Order** is
  Ascending or Descending.
- The check boxes are Fit thumbnails, Show name, Show resolution, Show extension, Show label and
  Show item count.
- **Animate zoom in the grid** is on by default. When it is on, the tiles move to their new places
  in 150 ms when the thumbnail size changes. When it is off, the grid shows the new layout at
  once.
- **Reset all views** clears the layout, the sort order and the display choices that are saved for
  the single views.

## Preview

- **Image** has **Scaling** (Smooth or Pixelated), **Default zoom** (Fit to window or Actual size)
  and **Show transparency grid**.
- **Video** has **Autoplay videos** and **Loop videos**.
- **Audio** has **Visualization** (None, Spectrum, Oscilloscope or Orbit).

## Shortcuts

- **Keyboard layout** is US (QWERTY) or EU (QWERTZ / AZERTY / Nordic). The EU layout uses other
  keys for the shortcuts that need the backtick, the backslash or the brackets, for example
  `Shift+F` for Fit to Window.
- Under it is the list of all shortcuts, with a search field. Click a binding and press new keys
  to change it. Picto does not accept keys that another shortcut already has, and it marks the
  row. Each shortcut can have a second binding.

This section is the reference for the keyboard shortcuts. The pages about the single features also
list the common ones.

## Notifications

A switch turns the pop-up notifications on or off. Check boxes choose which kinds are shown:
successful actions, information, warnings and errors.

## Auto-Import

- The **Auto-Import** switch turns all [watched folders](folders.md#watched-folders) on or off.
- **Manual Imports → Multiple files** is Ask every time, Group as collection or Keep separate.

## Inbox & Deleting

- **Rejected items** chooses what Reject does in the Inbox: **Move to Trash**, which is the
  default, or **Delete permanently**. Deleting does not put the item in the Trash.
- **Undo limit (media items)** is a number from 0 to 100,000. The default is 500. A delete of at
  most that many media items can be undone until Picto closes. The members of a collection count
  as media items. A larger delete asks for confirmation and cannot be undone. With 0, no delete
  can be undone.

## Subscriptions

- **Defaults for New Subscriptions** has the schedule (Daily by default), the posts per run (from
  1 to 10,000, 100 by default) and **Group multi-media posts** (on by default).
- **Maximum Inbox items** is 1,000 by default. The work of the subscriptions waits when the Inbox
  holds that many top-level items.

See [Subscriptions](subscriptions.md).

## Cloud

This section shows the state of the sync, the pending work, the recovery retention and the restore
history of the open library. See [Cloud Sync](cloud-sync.md).

## AI Models

In this section you download and remove the local tagging models, set the confidence levels and
turn on the auto-tagging of new imports. See [AI Tagging](ai-tagging.md).

## About

This section shows the version and the platform, a **Check Now** button for updates, the license
(MIT), and links to the project on GitHub, to the release notes and to the license.
