# Settings

[← User Guide](README.md)

Open Settings with `Ctrl+,` (`Cmd+,` on Mac). Settings are organized into panels.

## General

### Appearance

- **Theme** — Choose from: Auto, Dark, Blue, Purple, Gray, Light, Light Gray
  - Auto follows your system preference (defaults to dark)
  - Dark, Blue, and Purple are dark-themed
  - Light and Light Gray use light backgrounds
- **Zoom Level** — Scale the entire UI: 75%, 80%, 90%, 100%, 110%, 125%, 150%

### Grid Defaults

- **Default Layout** — Waterfall, Grid, or Justified
- **Thumbnail Size** — Default tile size in pixels (100-600, default 250)
- **Sort By** — Default sort field: Date Added, File Size, Rating
- **Sort Order** — Ascending or Descending

### Reverse Image Search

Enable or disable search engines for the right-click → Reverse Image Search menu:
- Google Lens
- TinEye
- SauceNAO
- Yandex Images
- Sogou
- Bing Visual Search

### Storage

View your library's storage statistics — total file count, active/inbox/trash breakdown, and total disk usage.

## Library

View information about the current library: name, path, and file count. See [Library Management](library-management.md) for library operations.

## Download Services

Configure subscription download behavior:

- Subscription downloads are serial. Picto preserves gallery-dl's provider-specific request timing; sources without a specific interval wait a random 0.5-2 seconds between requests to the same host. Failed requests retry with increasing backoff.
- **Batch Size** — Maximum files per subscription run. Set to unlimited or a specific number (1-5000).
- **Abort Threshold** — Stop a subscription run after this many consecutive already-downloaded files (1-500, default 10). Prevents re-scanning your entire download history.
- **Default Import Status** — Whether watched folders import as Inbox or Active by default.

## Duplicates

Configure [duplicate detection](duplicates.md):

- **Detection Similarity** — Minimum perceptual similarity to discover a pair (95-100%)
- **Review Similarity** — Minimum similarity to show in the review queue (95-100%)
- **Auto-Merge** — Toggle automatic merging of exact duplicates on import
  - **Similarity Threshold** — How similar files must be for auto-merge
  - **Require Matching Dimensions** — Only auto-merge if width and height match exactly
  - **Subscriptions Only** — Only auto-merge during subscription downloads (not manual imports)

## Shortcuts

A searchable reference of all [keyboard shortcuts](keyboard-shortcuts.md). Shortcuts are grouped by category and display platform-appropriate key symbols.

## Developer

Performance diagnostics for development:
- **SLO Check** — Shows latency percentiles (p50/p95/p99) for key operations
- Auto-refreshes every 10 seconds

## Danger Zone

Destructive operations:

- **Wipe Image Data** — Removes all images, videos, tags, and review items from the library. Subscription definitions are preserved. **This is irreversible.** A confirmation dialog is required.
