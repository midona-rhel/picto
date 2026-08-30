# Native Subscription Runtime

This document records the implemented architecture. The executable behavior contract is
`docs/SUBSCRIPTION_EXECUTION_CONTRACT.md`; finite unresolved release evidence belongs in the active
subscription PBIs.

## Production Path

Every source uses one path:

```text
subscription query
-> native Rust provider adapter
-> shared bounded HTTP runtime
-> one exposed source post
-> current-post media downloads
-> canonical SQLite ingest
-> durable post settlement acknowledgement
-> next source post
```

SQLite owns run, query, cursor, attempt, source identity, issue, and progress state. Providers own
only endpoint selection, bounded pagination/cursor decoding, response parsing, and media descriptor
construction. They do not own scheduling, download history, staging, ingestion, counters, retries,
or post settlement.

There is no gallery-dl, OF-Scraper, Python runtime, bridge protocol, extractor archive database, or
compatibility path in production or release packaging. Those projects remain behavioral references
only.

## Invariants

- At most one execution runs per subscription. A manual query run exclusively leases its
  subscription until it settles or is stopped.
- Each query exposes one post, records it traversed, downloads all usable current-post media,
  canonically ingests it, records it added or skipped, receives settlement acknowledgement, and
  only then exposes the next post.
- The configured limit counts added posts. No-media, inaccessible, deleted, and exact-duplicate
  posts are skipped and do not consume it.
- Media within the current post may download concurrently; later-post media may not be prefetched.
- Exact content hashes reuse canonical media facts and apply the accepted standalone-only metadata
  donation rules. They do not create another standalone root.
- Multi-media posts publish one collection atomically. Galleries are one finite atomic post.
- Staging is scoped by subscription, query, and run-query identity, and is removed after settlement,
  failure, startup recovery, or reset.
- Authentication remains the managed direct-site browser flow and OS credential store. Native
  adapters receive typed stored credentials scoped to approved first-party domains.
- Generic providers use randomized 0.5-2 second same-domain pacing. Explicit provider policies may
  match the authoritative source client when required.
- Restart may repeat an interrupted post, but stable source identity and content hashes keep the
  operation idempotent.

## Provider Matrix

The product registry and native registry are tested for exact equality:

- Search/tag: Danbooru, e621, Gelbooru, Idol Complex, Konachan, Pixiv, Rule34.xxx, Safebooru,
  Sankaku, Yande.re.
- Creator/account: Baraag, DeviantArt, Fur Affinity, Hentai Foundry, Newgrounds, OnlyFans, Patreon,
  pixivFANBOX, Pixiv users, SubscribeStar, Twitter/X.
- Gallery: one E-Hentai adapter supporting both E-Hentai and ExHentai hosts.
- Public archive mirrors: Pawchive, Coomer, Kemono.

Tumblr is intentionally absent from the product and native registries.

## Verification

Release evidence consists of:

1. Exact product/native registry and auth-contract tests.
2. Provider fixture tests for bounded discovery, one-post exposure, canonical tags, media identity,
   and site-specific parsing.
3. Shared engine tests for settlement ordering, skips, added budgets, partition cursors, recovery,
   cancellation, cleanup, and serial subscription leases.
4. Canonical ingest tests for exact-hash ownership and metadata behavior.
5. Live source certification using fresh temporary libraries, persisted blobs, restart, continuation,
   canonical metadata, collection ordering, and request pacing.
6. Release audit proving that no removed Python runtime or bridge is packaged.

## Database Boundary

There is no migration or conversion path. Pre-release development data is disposable. A new library
must match the current schema exactly; an incompatible library fails without mutation.
