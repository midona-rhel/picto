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
| Boorus and tag search | Danbooru, e621, Gelbooru, Rule34.xxx, Safebooru, Yande.re |
| Artists and accounts | DeviantArt, Fur Affinity, Hentai Foundry, Newgrounds, Pixiv (search and users), Twitter / X |
| Creator platforms | OnlyFans, Patreon, pixivFANBOX, SubscribeStar |
| Galleries and archives | E-Hentai / ExHentai galleries (via **Add Gallery**), Pawchive |

## Documentation

- [`CONTRIBUTING.md`](./CONTRIBUTING.md) says how issues, branches, pull requests and releases
  work.
- [`docs/architecture.md`](./docs/architecture.md) describes how the shipped system works.
- [`docs/user-guide/`](./docs/user-guide/) is the user guide.
- [`docs/RELEASE_READINESS.md`](./docs/RELEASE_READINESS.md) is the release procedure and its
  gates.
- [`experiments/gpui/README.md`](./experiments/gpui/README.md) describes the GPUI frontend, which
  is being built to replace the Electron frontend.

Work is tracked in [GitHub issues](https://github.com/midona-rhel/picto/issues). `main` holds
released code. Development happens on `dev`.

## The two frontends

The repository holds two apps that show the same libraries. Both use the same Rust crates for
everything below the user interface: `core` (the runtime, the work queues and media processing),
`picto-library` (the SQLite database and its queries) and `picto-sources` (the subscription
sources).

- **The Electron app** is the app that is released. Its user interface is a React app in `src/`,
  its main process is in `electron/`, and it reaches the Rust crates through a napi-rs addon in
  `native/picto-node/`.
- **The GPUI app** is in `experiments/gpui/`. It is written in Rust on GPUI, the user interface
  framework of the Zed editor, and runs the Rust crates in its own process. It is not released
  and has no installer yet. It has been used on macOS; on Windows and Linux it is built, tested
  and photographed by CI, and nobody has used it there. Issue
  [#163](https://github.com/midona-rhel/picto/issues/163) lists what remains before it replaces
  the Electron app.

A library that is open in one of the two apps is locked, and the other app refuses to open it.

## Development of the Electron app

The Electron app is built with Electron, React and the Rust crates named above. The Rust side
does the database work and the image processing. The React side draws the grid on a canvas.

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

### Quality gate

This command runs the checks that a release of the Electron app must pass:

```bash
npm run gate:alpha
```

### Packaging

This command builds the Electron app's package for the system it runs on:

```bash
npm run alpha:package
```

This command starts the unpacked app that the package command made and checks that it runs:

```bash
npm run alpha:smoke -- --report artifacts/alpha-smoke/local.json
```

The check starts the app with application data and library storage of its own, so it does not
touch the user's. It passes only when all of these hold: the native library starts, the main
window loads without a process, load or preload failure during a short wait, the native library
shuts down, and the app exits with code `0`.

## Development of the GPUI app

The GPUI app is a Cargo workspace of its own in `experiments/gpui/`, with its own `Cargo.lock`.
It needs the Rust toolchain that `rust-toolchain.toml` names and no Node.js.

```bash
cd experiments/gpui
cargo build --locked
cargo test --locked
```

[`experiments/gpui/README.md`](./experiments/gpui/README.md) has the rest:

- Section 2 says how the app is run safely. It opens a throwaway copy of a library, and it opens
  a library the user keeps only under the rules written there.
- Section 3 lists what a Linux or Windows machine needs to build it, and where the app looks for
  `ffmpeg`.
- Section 6 says how a change is checked, including the tool that plays a script in an
  off-screen window and saves pictures.
- Section 7 says what works, what was checked on each system, how the app differs from the
  Electron app on purpose, and what is open.

The `GPUI experiment` workflow builds and tests the app on Windows and Linux and saves pictures
of it. It is the only place where the app runs on those two systems. It runs for every pull
request into `dev` that changes the app or one of the shared crates, and it can be started by
hand for any branch from the Actions page.
