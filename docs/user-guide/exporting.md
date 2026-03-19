# Exporting

[← User Guide](README.md)

Picto can export files from your library in their original format or converted to a different format with optional resizing.

## Basic Export (Originals)

Press `Ctrl+E` (`Cmd+E` on Mac) to export selected files in their original format. Choose a destination folder and the files are copied as-is.

## Advanced Export

Press `Ctrl+Shift+E` (`Cmd+Shift+E` on Mac) for format conversion and resize options.

### Format

Choose the output format:

| Format | Notes |
|--------|-------|
| PNG | Lossless, larger files |
| JPEG | Lossy, configurable quality |
| WebP | Modern format, good quality/size ratio |
| AVIF | Newest format, best compression |

### Quality

For lossy formats (JPEG, WebP, AVIF), adjust the quality slider from 1-100. Higher values mean better quality but larger files. PNG is always lossless.

### Resize

Optionally resize exported files:

- Set a target **width** and/or **height** in pixels
- **Keep aspect ratio** — When enabled, the image scales proportionally. Only one dimension needs to be specified; the other is calculated automatically.

## Batch Export

Select multiple files before pressing the export shortcut. All selected files are exported with the same settings. A progress indicator shows the export status.

## Export Destination

You'll be prompted to choose a destination folder each time. Exported files are written to that folder with their original file names (or with the new extension if converting).
