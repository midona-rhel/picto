# Interface Overview

[← User Guide](README.md)

Picto's interface is divided into four main areas: the sidebar, the grid, the inspector panel, and the toolbar.

## Sidebar

The sidebar on the left provides navigation to all major views and organizational structures.

### System Views

- **All Active** — All accepted images and videos with active status (your library)
- **Inbox** — Newly imported files awaiting review (see [Inbox Workflow](inbox-workflow.md))
- **Uncategorized** — Active files not assigned to any folder
- **Untagged** — Active files with no tags
- **Trash** — Deleted files (can be restored or permanently removed)
- **Duplicates** — Duplicate detection and resolution (see [Duplicates](duplicates.md))

Each view shows a count badge with the number of files in that scope.

### Folders

A hierarchical folder tree for organizing files. Folders can be nested, reordered via drag-and-drop, and customized with icons and colors. See [Folders](folders.md).

### Smart Folders

Dynamic folders that automatically collect files matching tag-based rules. See [Smart Folders](smart-folders.md).

### Tags

Displays the total tag count. Click to open the tag management view. See [Tags](tags.md).

### Sidebar Job Status

The bottom of the sidebar shows the progress of running tasks — subscription downloads, duplicate scans, and other background operations.

## Grid

The main content area displays your files as thumbnails. It supports three layout modes:

- **Waterfall** (`Alt+2`) — Masonry layout with variable row heights (default)
- **Grid** (`Alt+1`) — Fixed-size uniform grid
- **Justified** (`Alt+3`) — Rows justified to fill the width

The grid supports:
- Single click to select, `Ctrl`+click to toggle, `Shift`+click for range, click-drag for marquee selection
- Arrow keys (or WASD) to navigate between thumbnails
- Double-click or `Enter` to open detail view
- Right-click for context menu with all file actions
- Thumbnail size adjustment via toolbar controls

See [Browsing and Navigation](browsing-and-navigation.md) for full details.

## Inspector Panel

The right panel shows metadata for the currently selected file(s):

- **Properties** — File name, size, dimensions, format, duration (video)
- **Rating** — 0-5 stars (click or press `0`-`5`)
- **Tags** — View, add, and remove tags with namespace color coding
- **Folders** — Which folders contain this file
- **Notes** — Free-text notes (hover to expand editor)
- **Source URLs** — Clickable links to original sources
- **Color Palette** — Dominant colors extracted from the image

The inspector width is resizable (200-600px). Toggle it with `Ctrl+Alt+2`.

## Toolbar

The top toolbar provides:

- **Back/Forward** — Navigate through view history (`Alt+Left/Right`)
- **Search** — Full-text search across file metadata (`Ctrl+F`)
- **Thumbnail size** — Increase/decrease tile size
- **View mode** — Switch between Grid, Waterfall, and Justified layouts
- **Sort** — Choose sort field and order (Date Added, File Size, Name, Resolution, Rating)
- **Filter** — Toggle the filter bar for rating, MIME type, and color filters

## Window Controls

On Windows and Linux, the custom titlebar includes minimize, maximize, and close buttons. On macOS, native traffic light buttons are used.

## Keyboard-Driven Workflow

Nearly every action in Picto has a keyboard shortcut. Press `Ctrl+K` to open the command palette, or see the [complete shortcut reference](keyboard-shortcuts.md).
