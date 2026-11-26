# Picto

A desktop application for downloading, organizing, and browsing large image and video libraries.
A library's data is stored in a SQLite database, and Roaring bitmaps keep the common queries fast.

## What Picto Does

- **Subscribe** — follow searches, artists and accounts on the sources below. Picto downloads new
  posts on a schedule or on demand.
- **Organize** — folders, smart folders, tags (with namespaces), star ratings, and color-based
  search. Drag and drop between folders, bulk-tag selections, and auto-tag with local AI models.
- **Browse** — waterfall, justified, and grid layouts, and a Quick Search panel that finds
  commands, folders, tags and items. A media view with zoom and pan. PDF, DjVu, EPUB and comic
  archives open in a document viewer. Duplicate detection with a side-by-side comparison view and
  Smart Merge.
- **Import** — drag files or folders into the app, point Picto at a directory to watch for new
  content, or import and export Picto Packs.
- **Own the library** — media and metadata remain in user-owned folders. Picto has no account
  service.

### Subscription sources

| Group | Sources |
|---|---|
| Boorus and tag search | Danbooru, e621, Gelbooru, Rule34.xxx, Safebooru, Yande.re |
| Artists and accounts | DeviantArt, Fur Affinity, Hentai Foundry, Newgrounds, Pixiv (search and users), Twitter / X |
| Creator platforms | OnlyFans, OnlyHaven, Patreon, pixivFANBOX, SubscribeStar |
| Galleries and archives | E-Hentai / ExHentai galleries (via **Add Gallery**), Pawchive |

## Documentation

- [`docs/user-guide/`](./docs/user-guide/) is the user guide.
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) says how issues, branches, pull requests and releases
  work.
- [`docs/architecture.md`](./docs/architecture.md) describes how the shipped system works.
- [`app/README.md`](./app/README.md) describes the app's code: how it is built, run and checked,
  how it is put together, and what is not done yet.
- [`docs/RELEASE_READINESS.md`](./docs/RELEASE_READINESS.md) is the release procedure and its
  gates.

Work is tracked in [GitHub issues](https://github.com/midona-rhel/picto/issues). Development
happens on `dev`, and releases are tagged there.

## What the repository holds

Picto is one program, written in Rust. It runs on macOS (Apple Silicon), Windows (x86-64) and
Linux (x86-64).

| Folder | What it is |
|---|---|
| `app/` | The app: the windows, the views and everything a user sees. It is written on GPUI, the user interface framework of the Zed editor. It is a Cargo workspace of its own, with its own `Cargo.lock`. |
| `core/` | The application core: the commands, the background workers, the import and media processing, the subscriptions' runs and the AI tagger. |
| `picto-library/` | The library: the SQLite database, its schema, its queries and the Roaring bitmap projections. |
| `picto-sources/` | The subscription sources: one provider for each site. |
| `scripts/` | The scripts of the build and the release, and the tools that measure the library crates. |
| `build/` | The icons of the app, of `.library` folders and of `.picto-pack` files. |
| `docs/` | The documents listed above, and the release notes of each version. |

`core`, `picto-library` and `picto-sources` are the members of the Cargo workspace at the
repository's root. The app uses `core` and `picto-library` as path dependencies and runs them in
its own process.

The version of Picto is written in one place, the `version` field of `package.json` at the
repository's root. The app's build reads it from there. `package.json` also names the npm packages
that the scripts in `scripts/` use; the app itself needs no Node.js.

## Development

The Rust toolchain is the one that `rust-toolchain.toml` names.

```bash
cd app
cargo build --locked
cargo test --locked
```

[`app/README.md`](./app/README.md) has the rest:

- Section 2 says how the app is run safely. A debug build opens a throwaway copy of a library,
  and it opens a library the user keeps only under the rules written there.
- Section 3 lists what a Linux or Windows machine needs to build the app, where the app looks for
  `ffmpeg`, and how the packages are made (`app/package.sh`).
- Section 6 says how a change is checked, including the tool that plays a script in an
  off-screen window and saves pictures.
- Section 7 says what works, what was checked on each system, and what is open.

The crates of the root workspace are checked from the repository's root:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --lib --tests --locked
```

The scripts have tests of their own, and the source audit checks what must not be committed:

```bash
npm ci
npm test
node scripts/release-audit.mjs
```

CI runs all of this for every pull request, and it builds and tests the app on Linux and Windows
(the `verify` workflow). CI has no macOS job, so the app is built and tested on macOS before a
pull request is opened.
