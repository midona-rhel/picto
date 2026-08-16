# Picto

A desktop application for downloading, organizing, and browsing large image and video libraries.
SQLite stores library truth and Roaring bitmap projections keep common queries fast.

## What Picto Does

- **Subscribe** — follow Pixiv searches and users plus Gelbooru and Danbooru searches. Picto downloads new posts on a schedule or on demand.
- **Organize** — folders, smart folders, tags (with namespaces), star ratings, and color-based search. Drag and drop between folders, bulk-tag selections, and auto-tag with AI.
- **Browse** — waterfall, justified, and grid layouts. Full-screen media view with zoom/pan. Strip view for reading comics and image sequences. Duplicate detection with visual similarity matching.
- **Import** — drag files or folders into the app, or point Picto at a directory to watch for new content.
- **Own the library** — media and metadata remain in user-owned folders. Optional folder sync uses desktop software such as Google Drive or Dropbox as transport; Picto has no cloud account service.

## Documentation

Current product and release documentation lives in `docs/`:

- [`docs/RELEASE_COMPLETION_PLAN.md`](./docs/RELEASE_COMPLETION_PLAN.md) — executable release backlog
- [`docs/pbis/active-alpha/`](./docs/pbis/active-alpha/) — current blockers only
- [`docs/user-guide/`](./docs/user-guide/) — user guide

## Development

Picto is built with Electron + React + a Rust core (via napi-rs). The Rust backend handles SQLite, image processing, and all heavy lifting. The frontend is a React app with canvas-based rendering for the grid.

### Prerequisites

- Node.js 20+
- Rust toolchain (stable)
- npm

### Setup

```bash
npm ci
cd native/picto-node && npm ci && cd ../..
```

### Run in development

```bash
npm run dev:electron
```

### Build the native addon

```bash
cd native/picto-node && ./node_modules/.bin/napi build
```

## Quality Gates

Alpha blocking gate:
```bash
npm run gate:alpha
```

## Packaging

Local package:
```bash
npm run alpha:package
```

Packaged smoke (requires the unpacked product created by the package command):
```bash
npm run alpha:smoke -- --report artifacts/alpha-smoke/local.json
```

The smoke launches the unpacked product with isolated app data and library storage. It passes only
when native library initialization succeeds, the main window loads without process/load/preload
failures during a short settle period, native shutdown succeeds, and the app exits with code `0`.
