# Tags

[← User Guide](README.md)

Tags are the primary way to describe and categorize your files in Picto. Tags use a **namespace:subtag** format that helps organize them into meaningful groups.

## Tag Format

Tags follow the pattern `namespace:subtag`:

- `creator:john_doe` — The artist or creator
- `character:some_character` — A character depicted
- `series:my_series` — A series or franchise
- `meta:wallpaper` — Meta information about the file

Tags without a namespace are stored as unnamespaced (just the subtag part).

## Applying Tags

There are several ways to tag files:

### Quick Tag (T key)

1. Select one or more files in the grid
2. Press `T` to open the tag selector
3. Search and select tags to apply
4. Close the selector — tags are applied immediately

### Inspector Panel

Click the tags section in the [inspector panel](interface-overview.md#inspector-panel) to add or remove tags for the selected file.

### Context Menu

Right-click a file or selection and choose **Add Tags**.

### Copy and Paste Tags

- `Ctrl+Shift+C` — Copy tags from the selected file(s)
- `Ctrl+Shift+V` — Paste copied tags onto the current selection

This is useful for applying the same set of tags to multiple files.

## Tag Manager

The tag manager provides a full view of all tags in your library. Access it by clicking **Tags** in the sidebar.

Features:
- **Browse** all tags with namespace grouping
- **Search** by tag name
- **Rename** tags (`Ctrl+R`) — all files with that tag are updated
- **Delete** tags — removes the tag from all files
- **Merge** tags — combine two tags into one, updating all files

## Namespaces

Namespaces group related tags. Each namespace gets a distinct color in the UI:

| Namespace | Purpose |
|-----------|---------|
| `creator` | Artists, photographers, authors |
| `studio` | Production studios |
| `series` | Franchises, shows, games |
| `character` | Characters depicted |
| `person` | Real people |
| `species` | Species or creature types |
| `meta` | Meta tags (wallpaper, screenshot, etc.) |
| `system` | System-managed tags |

Tags are sorted by namespace in the inspector: creator → studio → series → character → person → species → meta → system → unnamespaced.

## Batch Tagging

Select multiple files with `Ctrl+click`, `Shift+click`, or `Ctrl+A`, then press `T` to tag them all at once. Tags are applied to every selected file.
