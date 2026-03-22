# Picto

A desktop application for aggregating, organizing, and browsing large image and video collections. Picto downloads media from online sources via subscriptions, stores everything in a local library, and provides powerful tools for tagging, searching, and managing your collection. It is not a sharing platform — everything stays on your machine.

## What Picto Does

- **Download & aggregate** — subscribe to artists, tags, or feeds across dozens of sites (Pixiv, Gelbooru, Danbooru, Twitter/X, DeviantArt, Patreon, and more). Picto fetches new content automatically on a schedule.
- **Organize** — folders, smart folders, tags (with namespaces), star ratings, and color-based search. Drag and drop between folders, bulk-tag selections, and auto-tag with AI.
- **Browse** — waterfall, justified, and grid layouts. Full-screen media view with zoom/pan. Strip view for reading comics and image sequences. Duplicate detection with visual similarity matching.
- **Import** — drag files or folders into the app, or point Picto at a directory to watch for new content.
- **Everything local** — libraries are self-contained folders on disk. No cloud, no accounts, no uploads.

## Documentation

The user manual and feature documentation live in the `docs/` directory:

- [`docs/pbis/active-alpha/`](./docs/pbis/active-alpha/) — current alpha backlog and feature specs
- [`docs/pbis/archive/`](./docs/pbis/archive/) — completed feature specs

## Development

Picto is built with Electron + React + a Rust core (via napi-rs). The Rust backend handles SQLite, image processing, and all heavy lifting. The frontend is a React app with canvas-based rendering for the grid.

### Prerequisites

- Node.js 20+
- Rust toolchain (stable)
- Yarn

### Setup

```bash
yarn install
cd native/picto-node && yarn install && cd ../..
```

### Run in development

```bash
yarn dev:electron
```

### Build the native addon

```bash
cd native/picto-node && ./node_modules/.bin/napi build
```

## Quality Gates

Alpha blocking gate:
```bash
yarn gate:alpha
```

## Packaging

Local package:
```bash
yarn alpha:package
```

Local smoke test:
```bash
yarn alpha:smoke -- --platform local --report artifacts/alpha-smoke/local.json
```
