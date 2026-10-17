# Picto Native Sources

`picto_sources` replaces gallery-dl and OF-Scraper with composable Rust source capabilities. It
does not own authentication, SQLite, canonical media ingest, thumbnails, or UI progress.

## One Execution Path

The one-post settlement rule, post-limit accounting, and multi-file post behavior are defined once
in [`docs/architecture.md`](../docs/architecture.md#subscriptions). This crate implements its source
side. Every provider is assembled from the same layers:

1. `NativeSourceAdapter` validates a query and maps one bounded source page into `SourcePost`
   values.
2. `SourceSession` exposes one post at a time (see the settlement rule linked above).
3. `PartitionedSourceSession` runs independent feeds under one query-level budget.
4. `PostDownloader` downloads only the active post, optionally in parallel within that post, and
   reports every completed file.
5. `picto_core::native_source_import` probes and hashes downloads and creates the sole canonical
   standalone/collection ingest payload.
6. The durable subscription worker records traversal, download, ingest, added/skipped outcome,
   cursor, and cleanup before it asks the adapter for another post.

Provider code may define only endpoint, response schema, cursor shape, partition list, and field
mapping. A provider-specific scheduler, downloader, temporary-file owner, tag store, duplicate
path, or ingest implementation is a design defect.

## Implemented Shared Capabilities

- Bounded JSON page requests and typed response normalization.
- Numeric keyset and bounded opaque cursors.
- Reserved query-control validation.
- Global concurrency with independently configured per-domain pacing, timeout, retries, and
  `Retry-After` support.
- Existing Picto-managed headers and cookies passed in as opaque request credentials.
- Current-post-only parallel downloads and per-file progress.
- Safe staging names and atomic `.picto-part` publication.
- Canonical tag namespace mapping with unknown source groups falling back to general.
- Shared HTML, BBCode, and DText cleanup.
- Standalone versus collection assembly through one canonical ingest mapper.
- Independent partition cursors with one query-level settled-post budget.

The crate is integrated with Picto's durable worker, reset/recovery path, and exact product
registry. Provider fixture tests and live certification use that production path.

## Provider Families

| Family | Providers | Reused composition |
|---|---|---|
| Booru APIs | Danbooru, e621, Gelbooru, Rule34, Safebooru, Yande.re | JSON/XML page request, keyset/page cursor, one-media post mapping, category tags |
| Creator APIs | Pixiv search, Pixiv user, FANBOX | JSON pagination, multi-media post mapping, creator/general tags |
| Paid creator feeds | Patreon, SubscribeStar, OnlyFans | Opaque cursors, access warnings, multi-media posts; OnlyFans adds purchases/messages/feed partitions and DRM media resolution |
| Social/account feeds | DeviantArt, Twitter/X, Newgrounds, Fur Affinity, Hentai Foundry | Account/query normalization, HTML or JSON discovery, post-detail mapping |
| Gallery/archive feeds | E-Hentai/ExHentai, Pawchive | Paged gallery/archive discovery, atomic collection assembly, bounded media downloads |

Family fixtures prove shared composition; every provider also owns focused parser and cursor tests.

Tumblr is intentionally not part of the native registry. gallery-dl and OF-Scraper are behavioral
references only and are not production or packaged dependencies.

When a source changes or starts producing incorrect results, follow
[`docs/SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md`](../docs/SOURCE_INTEGRATION_BREAKAGE_PLAYBOOK.md).
It defines the reference-comparison, fixture, authentication, cursor, live-certification, and
release-audit workflow without reintroducing a sidecar runtime.

## Cutover Rule

The native registry and worker are the sole production authority. The system does not dual-write,
reverse-map DTOs, convert development download history, or preserve removed runtime state.
