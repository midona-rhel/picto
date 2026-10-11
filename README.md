# Picto

A desktop application for downloading, organizing, and browsing large image and video libraries.
SQLite stores library truth and Roaring bitmap projections keep common queries fast.

## What Picto Does

- **Subscribe** — follow searches, artists and accounts on the sources below. Picto downloads new
  posts on a schedule or on demand.
- **Organize** — folders, smart folders, tags (with namespaces), star ratings, and color-based
  search. Drag and drop between folders, bulk-tag selections, and auto-tag with local AI models.
- **Browse** — waterfall, justified, and grid layouts. Full-screen media view with zoom/pan. Comic
  archives open in a paged viewer. Duplicate detection with a side-by-side comparison view and
  Smart Merge.
- **Import** — drag files or folders into the app, point Picto at a directory to watch for new
  content, or import and export Picto Packs.
- **Own the library** — media and metadata remain in user-owned folders. Cloud sync
  (**experimental**) uses the Google Drive or Dropbox desktop folder as transport; Picto has no
  cloud account service.

### Subscription sources

| Group | Sources |
|---|---|
| Boorus and tag search | Danbooru, e621, Gelbooru, Idol Complex, Konachan, Rule34.xxx, Safebooru, Sankaku, Yande.re |
| Artists and accounts | Baraag, DeviantArt, Fur Affinity, Hentai Foundry, Newgrounds, Pixiv (search and users), Twitter / X |
| Creator platforms | OnlyFans, Patreon, pixivFANBOX, SubscribeStar |
| Galleries and archives | E-Hentai / ExHentai galleries (via **Add Gallery**), Pawchive |

## Documentation

Current product and release documentation lives in `docs/`:

- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — issues, branches, pull requests and releases
- [`docs/architecture.md`](./docs/architecture.md) — how the shipped system works
- [`docs/user-guide/`](./docs/user-guide/) — user guide

Work is tracked in [GitHub issues](https://github.com/midona-rhel/picto/issues). `main` holds released
code; development happens on `dev`.

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
