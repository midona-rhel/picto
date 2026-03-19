# Duplicate Detection

[← User Guide](README.md)

Picto uses perceptual hashing to detect near-duplicate images. This finds visually similar images even if they differ in format, resolution, or compression level.

## How It Works

When files are imported, Picto computes a perceptual hash (pHash) for each image. A BK-tree data structure enables efficient similarity lookups across your entire library.

Two images are considered potential duplicates if their perceptual similarity exceeds the configured threshold (default: 95%).

## Scanning for Duplicates

Navigate to **Duplicates** in the sidebar. If no pairs are shown, click **Scan for Duplicates** to analyze your library.

Picto also runs periodic background scans (every 5 minutes while the duplicates view is open) to catch newly imported duplicates.

## Reviewing Pairs

The duplicate review interface shows two images side by side:

- **Left image** — File A of the duplicate pair
- **Right image** — File B of the duplicate pair
- **Center column** — Resolution action buttons
- **Similarity badge** — Percentage match (color-coded: red for 99%+, orange for 95%+)

Below each image, metadata shows: file name, dimensions, file size, format, and tag count.

Both images are scaled to the same visual size regardless of their actual resolution. This means a lower-quality duplicate will appear pixelated compared to the higher-quality original, making quality differences immediately visible.

## Zoom and Pan

Zoom into both images simultaneously to compare fine details:

- **Scroll wheel** — Zoom in/out (focal-point zoom follows your cursor)
- **Click and drag** — Pan the view
- **F key** — Reset to fit-to-pane

Zoom works on whichever image your cursor is over. Both images pan and zoom together so the same region is always visible in both panes.

A navigator minimap appears when zoomed in, showing your current viewport position.

## Resolution Actions

| Key | Action | What Happens |
|-----|--------|--------------|
| `S` | Smart Merge | Keeps the higher-quality file, merges tags and metadata from both |
| `L` | Keep Left | Keeps left image, moves right to trash |
| `R` | Keep Right | Keeps right image, moves left to trash |
| `N` | Not Duplicate | Marks the pair as not duplicate (won't appear again) |
| — | Keep Both | Keeps both files, marks pair as resolved |

After resolving, the next pair loads automatically.

| Key | Action |
|-----|--------|
| `Left Arrow` | Previous pair |
| `Right Arrow` | Next pair |

## Undo

All resolution actions support undo (`Ctrl+Z`). This restores the trashed file and re-opens the pair for review.

## Settings

In [Settings](settings.md) → Duplicates:

- **Detection Similarity** — Threshold for discovering pairs (95-100%)
- **Review Similarity** — Minimum similarity to show in review queue
- **Auto-Merge** — Automatically merge exact duplicates on import
  - **Require Matching Dimensions** — Stricter safety check
  - **Subscriptions Only** — Limit auto-merge to subscription imports
