# PBI-575: Finish subscription sources

## Priority

P1 release blocker.

## Observed gap

The durable subscription runtime works, but Picto's supported source catalog is not fully certified.
The production registry contains 18 sources; the matrix below is limited to those sources. Existing
strict artifacts that report collection aggregates predate the flattened media model and are
historical diagnostics, not current certification. Shallow download reports are also not
certification, and no source currently has durable evidence for the complete current Electron
workflow.

Source certification may proceed against the flattened media model. Existing evidence created before
that model landed is historical and does not certify current behavior.

## Behavior

A subscription is a top-level item, contains one or more queries, and owns one manual, daily, weekly,
or monthly schedule. There are no subscription groups, group-level schedules, or group run controls.
A run durably queues every enabled query, streams completed posts into the durable ingest
queue, and finishes only after its downloads and ingest rows are terminal. Restart, stop, retry,
resume, and replay must not lose work or create duplicate media.

Each source is only an adapter. Gallery-dl-backed adapters must emit the same normalized post/media
events and enter the same ingest queue. A future OnlyFans adapter, if implemented, must not run
through gallery-dl or reuse its configuration, authentication, pagination, archive, or download
behavior. There is still no source-specific database insertion path.

Every file in a multi-image or multi-file post is imported as an independent image or video media
entity. Each entity receives the shared source-post metadata and its source order. No aggregate post
entity, hidden group, placeholder, or automatic per-post folder is created.

Future grouping or rearrangement may be represented by a dedicated external media manifest or file
format only. The current model contains no grouping abstraction, placeholder, hidden group, or
extension hook.

## Source acceptance

A source may appear in the application only after all of these pass:

1. Query or account input builds the correct canonical source URL.
2. Login opens the source's real website in a Picto-managed browser. Picto captures the resulting
   session and stores it in the OS credential store; users never paste passwords, cookies, tokens,
   or API keys into Picto.
3. Post identity, creator, source URLs, timestamps, tags, rating, and child ordering normalize
   correctly for representative real content.
4. Single-image and multi-image posts use the canonical ingest; every file is independent and carries
   shared source-post metadata and source order, with no hidden group or automatic folder.
5. Pagination, interruption, restart, resume, rerun, and archive replay neither skip nor duplicate
   media.
6. Failures remain persisted and understandable instead of silently completing.
7. The strict backend certification and a real Electron workflow both pass.

Credentials and downloaded user content are never committed. Unattended certification uses an
explicit local plaintext fixture loaded into a process-only test credential store; Picto users still
use the OS keychain. Attended paid-source login is available for Patreon, SubscribeStar, Fanbox, and
OnlyFans. Complete every other source without asking the user for credentials.

## Source matrix

| Source | Adapter | State and evidence |
| --- | --- | --- |
| Danbooru | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/danbooru-landscape-safe.json` |
| Gelbooru | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/gelbooru-huffslove.json` |
| Pixiv search | gallery-dl | Shallow-only evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-pixiv.json` |
| Pixiv user | gallery-dl | Shallow-only evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-pixivuser.json` |
| Rule34.xxx | gallery-dl | Shallow-only evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-rule34.json` |
| ArtStation | gallery-dl | Shallow-only public-profile evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-artstation.json` |
| Webtoons | gallery-dl | Historical episode evidence predates flattening; current 100-post backend and Electron certification pending. Evidence: `artifacts/subscription-certification/webtoons-live-with-yourself-1.json` |
| Hentai Foundry | gallery-dl | Historical 100-post and attended-login evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/hentaifoundry-public-100.json` |
| Baraag | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/baraag-public-100.json` |
| DeviantArt | gallery-dl | Shallow-only public-profile evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-deviantart.json` |
| Tumblr | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/tumblr-nasa-100.json` |
| Fur Affinity | gallery-dl | Historical 100-post direct-login evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/furaffinity-mirlinthloth-100.json` |
| Idol Complex | gallery-dl | Shallow-only public-path evidence; strict backend and Electron workflow pending. Existing report: `artifacts/site-verification/report-idolcomplex.json` |
| Sankaku | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/sankaku-broad-100.json` |
| Yande.re | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/yandere-landscape-100.json` |
| Konachan | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/konachan-landscape-100.json` |
| Safebooru | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/safebooru-1girl.json` |
| e621 | gallery-dl | Historical 100-post evidence predates flattening; current backend and Electron certification pending. Evidence: `artifacts/subscription-certification/e621-solo-canine-100.json` |

The registry has 18 entries: Pixiv search, Pixiv user, Gelbooru, Rule34, Danbooru, ArtStation,
Webtoons, Hentai Foundry, Baraag, DeviantArt, Tumblr, Fur Affinity, Idol Complex, Sankaku,
Yande.re, Konachan, Safebooru, and e621. The matrix intentionally does not list inactive or
unimplemented sources such as Twitter/X, Patreon, SubscribeStar, Nijie, Fantia, Fanbox, Coomer,
Pawchive, Pawoo, Instagram, or OnlyFans. Attended paid-source login is available only for Patreon,
SubscribeStar, Fanbox, and OnlyFans; that availability is not evidence that those sources are
implemented or certified.

## Implementation tickets

### S6.1 Shared source contract and strict harness

- Keep one explicit source registry; do not restore a generic fallback adapter.
- Keep subscriptions as the only list, schedule, and run unit; do not restore subscription groups.
- Keep the strict certification as the only readiness authority.
- Make the verifier enumerate the intended matrix instead of hardcoding a subset of sources.
- Record exact pass evidence per source without secrets or downloaded content.

### S6.2 Public booru family

Danbooru, Gelbooru, Yande.re, Konachan, Safebooru, and e621 have strict 100-post backend evidence
with explicit URL, identity, metadata, ordering, pagination, and resume checks. Rule34 has only
shallow evidence and remains pending strict certification. Shared booru parsing is used only where
the contracts are actually identical.

### S6.3 Account and social family

ArtStation, Baraag, DeviantArt, Tumblr, Fur Affinity, Idol Complex, and Sankaku have only shallow
evidence. Their strict backend and Electron workflows remain pending, including authenticated or
mature/private paths where applicable. Twitter/X, Instagram, and Pawoo are not in the active registry
and are not release work in this PBI. Every source gets the same direct-site login/session path,
including sources that may also run anonymously. Remove manual credential forms and paste flows.

### S6.4 Creator and paywall family

Hentai Foundry has historical 100-post and Electron evidence, but needs recertification against the
flattened model. Webtoons has historical 1-episode evidence but still needs a current 100-post
certification and Electron workflow proof. Patreon, SubscribeStar, Fanbox, and
OnlyFans are paid-source login targets for attended testing but are not active registry entries;
their implementations and certifications remain future work. Nijie, Fantia, Coomer, and Pawchive
are not in the active registry and are not release work in this PBI. Prove creator identity and
multi-image post boundaries rather than treating downloaded files as unrelated results.

### S6.5 Sankaku family

Sankaku and Idol Complex have shallow public-path evidence only. Their strict backend and Electron
workflows remain pending. Both use gallery-dl's source-native opaque keyset cursor rather than
numeric offsets, so new posts cannot shift an interrupted initial sync past unseen media.

### S6.6 Paid-source scope

Patreon, SubscribeStar, Fanbox, and OnlyFans may be tested with attended credentials supplied by the
user, but none is currently in the active registry. OnlyFans will require a dedicated runner rather
than gallery-dl, with separate image, video, mixed, locked/unavailable media, interruption, restart,
and rerun certification. Do not advertise any paid source before its implementation and strict
backend plus Electron evidence exist.

### S6.7 Certification and UI exposure

Certify sources in bounded batches. Expose each source only after its strict backend evidence and
real Electron workflow evidence pass. Delete this PBI only when every active registry entry is
certified; inactive and unimplemented sources are tracked separately rather than represented as
stale matrix rows.

## Verification

For every source, use a small known query or account and verify:

- fetched post count and downloaded media count explain any difference
- images and videos appear in Inbox, never All, until accepted
- post identity, shared metadata, and source order survive restart on independent media entities
- an interrupted run resumes without silently stopping or replaying imported media
- a completed rerun reports up to date and creates no duplicate entities
- login uses the real source page in a Picto-managed browser and the captured session remains local
- run progress, terminal state, Health, and History agree

## Out of scope

- a second subscription runtime or source-specific ingest path
- one PBI per source
- guided onboarding
- the general UI visual-polish pass
