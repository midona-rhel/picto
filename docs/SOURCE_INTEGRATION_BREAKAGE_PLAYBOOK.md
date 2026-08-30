# Source Integration Breakage Playbook

Use this document when a subscription provider stops discovering posts, skips media that is not in
the library, repeats pages, downloads the wrong asset, or fails authentication. The current
[gallery-dl extractors](https://github.com/mikf/gallery-dl/tree/master/gallery_dl/extractor) are the
compatibility specification for ordinary providers. The current
[OF-Scraper implementation](https://github.com/datawhores/OF-Scraper/tree/main/ofscraper) is the
compatibility specification for OnlyFans. Do not invent an alternative protocol when either
reference already handles the case: reproduce its observable request and extraction behavior in
the native Rust adapter.

These projects remain references, not production dependencies. They must not be reintroduced as
Python processes, sidecars, packaged binaries, databases, or fallback execution paths.

## Ownership Map

- `picto-sources/src/providers/<site>.rs`: endpoint selection, response parsing, cursor shape,
  source identity, tags, and media descriptors.
- `picto-sources/src/http.rs`: shared request policy, pacing, retries, redirects, credentials, and
  downloads.
- `picto-sources/src/adapter.rs`: provider contract and native registry.
- `core/src/native_source.rs`: adapter execution and canonical ingest handoff.
- `core/src/subscription_runtime.rs` and `core/src/library_subscription_state.rs`: durable runs,
  settlement, reset, recovery, and counters.
- `electron/windows/authSites.mjs`, `authSessions.mjs`, and `externalOnlyFansAuth.mjs`: interactive
  authentication and credential capture.
- `picto-sources/tests/fixtures/<site>/`: sanitized provider responses used by parser tests.
- `core/tests/subscription_source_readiness.rs`: fresh-library live certification.

Provider code must not acquire its own scheduler, downloader, history database, staging owner, or
ingest path. If multiple providers need a behavior, fix it once in the shared HTTP or execution
layer.

## Diagnose the Broken Boundary First

Classify the first incorrect observation before editing code:

1. **Query validation** — the query is rejected or normalized to the wrong creator, tag, or URL.
2. **Authentication/preflight** — the saved credential is incomplete, scoped to the wrong domain,
   expired, or valid but lacks access.
3. **Request behavior** — endpoint, method, parameters, headers, browser identity, pacing, retry, or
   redirect behavior differs from the source.
4. **Discovery/parser** — the response succeeds but posts, media, metadata, or the end-of-history
   signal are mapped incorrectly.
5. **Cursor/partition** — pages repeat, posts disappear, or reset retains provider progress.
6. **Media resolution/download** — a preview, intermediary page, expired URL, or inaccessible
   rendition is mistaken for the final media.
7. **Canonical ingest/settlement** — valid downloaded bytes are skipped, grouped incorrectly, or a
   cursor advances before the current post settles.

Useful symptoms:

- `401` usually means missing or expired authentication; `403` may mean either authentication or
  genuine account access. Do not relabel access denial as an empty feed.
- `429` is a request-policy problem. Honor `Retry-After` and match the reference client's pacing;
  do not add an unbounded provider loop.
- Successful traversal with every post skipped requires inspecting source-post attempts, stable
  item keys, canonical hashes, and actual blob presence. Do not assume the provider returned no
  media.
- Repeated posts usually mean the provider cursor represents the next page rather than the current
  post, or that the cursor was committed before settlement.
- One item from a multi-media post usually means the parser emitted one descriptor instead of every
  usable media entry. A post is ingested as one atomic standalone item or collection.

## Port the Reference Behavior

The default repair method is to find the matching upstream path and port its behavior directly.
Start from the latest working upstream revision, record its commit SHA in the fix, and follow the
whole path rather than copying one endpoint in isolation. That includes query normalization,
authentication, request construction, pagination, media resolution, ordering, throttling, retries,
and terminal conditions.

Picto deliberately differs only after extraction: the shared Rust runtime owns durable settlement,
canonical ingest, blob storage, progress, and reset. Translate the reference into that contract, but
do not redesign the provider protocol. Because gallery-dl is GPL-licensed, reproduce behavior and
data flow without copying its source text into this MIT-licensed repository.

### Gallery-dl-backed behavior

For ordinary providers, inspect `gallery_dl/extractor/<site>.py`, its base class, and every helper or
downloader it calls. Run the same known query through gallery-dl when permitted. Picto should match:

- query normalization and accepted URL forms;
- metadata and media endpoints, parameters, and pagination direction;
- required first-party headers, cookies, user agent, redirects, and referrer;
- request spacing and handling of rate limits;
- post identity, media order, canonical URL, timestamps, descriptions, and tag namespaces;
- original-media selection, intermediary-page resolution, and fallback order;
- empty, inaccessible, deleted, and end-of-history behavior.

Never paste session cookies, authorization headers, OAuth secrets, signed URLs, or private response
payloads into an issue, fixture, test output, or commit. Reduce a captured response to the smallest
sanitized fixture that still reproduces the parser or cursor defect. Translate the complete behavior
into the existing Rust adapter structure. If Picto intentionally cannot match it, document the
concrete product or security reason and return an explicit unsupported/error outcome instead of
silently doing something different.

### OnlyFans behavior

Follow OF-Scraper's `ofscraper` request, API, authentication, and media paths, including every helper
they depend on. Use the current web client and dynamic-rules projects for values OF-Scraper obtains
at runtime. The native adapter remains the sole implementation, but its observable behavior should
match the reference. Verify these boundaries separately:

- Login capture keeps `sess`, `auth_id`, `x-bc`, and the matching browser user agent from one
  authenticated session. Credentials remain scoped to approved first-party domains.
- API requests use the current app token and a signature generated from validated dynamic rules.
  The adapter currently loads a primary and fallback rules source and caches valid rules for a
  bounded period.
- The `purchased`, `messages`, and `feed` partitions have independent durable cursors. The feed
  walks timeline, archived, pinned, and streams in deterministic order.
- Stable post and media identities deduplicate the same asset across partitions without dropping
  provenance or distinct media from a post.
- Locked or inaccessible media produces an explicit warning or skipped outcome. It must not report
  a false success.
- Direct images, audio/video, and clear segmented media use the shared download path. DRM-protected
  media remains an explicit CDM/device boundary; do not silently substitute a preview.

OnlyFans request signing changes frequently. Port the reference's current rule parsing and signing
flow into the existing adapter and add a sanitized dynamic-rules fixture. Do not hard-code a freshly
observed signature or store a user's signed request.

## Safe Fix Workflow

1. Reproduce with one known query and note the exact phase, status, and first wrong post. Prefer a
   fresh temporary library so persisted cursors and canonical hashes cannot disguise the defect.
2. Compare the same query with the appropriate reference and write down the smallest behavioral
   difference.
3. Add or update a sanitized response fixture before changing the parser. Include the relevant
   boundary: next page, empty page, missing field, locked media, duplicate media, or changed rule.
4. Make the smallest native change in the owning layer. Keep provider-specific behavior inside its
   adapter; move only genuinely shared behavior into the HTTP/session layer.
5. Prove query normalization, parsing, media order, source identity, cursor round-trip/bounds, and
   end-of-history with focused tests.
6. Prove production behavior with a fresh-library run. Check persisted blobs, standalone versus
   collection shape, metadata, restart/continuation, reset replay, request pacing, and idempotence.
7. Run the release audit to ensure no removed Python runtime or sidecar returned.

Focused commands:

```sh
cargo test --manifest-path picto-sources/Cargo.toml providers::<site>::tests
cargo test --manifest-path core/Cargo.toml native_source
npm run subscriptions:verify-sites -- --site <site-id> --query "<known-query>" --post-limit 5
npm run release:audit
```

The live verifier supports `--credential-file <path>` for an explicitly supplied local credential
fixture. It otherwise uses anonymous access. Add `--allow-keychain` only for an attended local run;
never make keychain access or private credentials part of automated tests.

## Required Regression Evidence

A provider repair is complete only when the relevant evidence exists:

- a focused sanitized fixture reproduces the upstream response shape;
- parser and cursor tests cover the failure and the end boundary;
- authentication changes have Electron session tests and preserve credential scoping;
- a fresh-library run materializes the expected media and blobs;
- restart continues from the last settled post;
- reset clears native run/cursor history and starts discovery from the beginning, while canonical
  hash reuse still prevents duplicate library roots;
- multi-media posts preserve every usable item in source order;
- request traces show bounded pacing without containing credentials;
- unrelated providers and the release audit remain green.

Do not declare success from `posts traversed` alone. Confirm `posts added`, `posts skipped`, retained
files, canonical roots, blob files, and the terminal reason for every skipped or failed post.
