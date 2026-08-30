# Subscription Execution Contract

All subscription providers implement the same ordered query pipeline. Provider adapters normalize
authentication, pagination, post metadata, and media URLs; they do not define different progress or
settlement semantics.

## Post Boundary

For each query, Picto performs these steps in order:

1. Select the next source post from the provider's metadata page.
2. Record the post as traversed as soon as its metadata is available.
3. Determine whether the post contains usable media.
4. If it has no usable media, record it as skipped and settle the post.
5. Download every usable media file for the current post. Files within this post may download
   concurrently.
6. Persist the downloaded bytes and ingest the post as one standalone root or collection.
7. After canonical ingestion succeeds, increment posts added.
8. Only then request the next source post.

A provider may prefetch a bounded page of post metadata when its native API requires pagination,
but it must not publish a later traversal or download or ingest media belonging to a later post
while the current post is unsettled.

## Limits And Progress

`Posts per run` is the maximum number of added source posts for each query in one run. Skipped posts
advance the cursor but do not consume the added-post budget. Failed or interrupted posts pause the
run and do not advance the cursor. Once the added limit is reached, Picto does not request metadata
for another post.

The persisted counters mean:

- `Posts traversed`: source posts whose metadata Picto inspected.
- `Posts added`: source posts whose complete usable media set reached canonical library state.
- `Posts skipped`: source posts settled without a new visible item.
- `Files downloaded`: usable media files whose bytes were downloaded and persisted.

At all times, `posts traversed <= posts added + posts skipped + 1`. The optional extra post is the
single post currently downloading or ingesting; it can never exceed the configured run limit.

Progress is published while each current post downloads and ingests. A run cannot report a post as
added before canonical ingestion succeeds. Retry and restart resume from the last settled post;
source identity keeps repeated work idempotent.

## Provider Requirements

- Every native adapter returns a bounded metadata window and exposes one post to the shared session
  at a time. The session refuses to expose another post until Picto acknowledges settlement.
- Gallery imports treat the entire gallery as the one current post.
- ZIP attachments from every provider use the shared bounded extractor and ingest accepted members
  as the current post's collection; adapters must not discard ZIPs themselves. The archive and its
  accepted expanded contents are each capped at 1 GiB, with a 512 MiB per-entry limit.
- Query providers continue until the added-post budget is reached or source history ends.
- Providers own endpoint and response-shape details only. The shared Rust HTTP runtime, downloader,
  staging owner, canonical ingest path, and settlement engine own all execution behavior.

## Adapter Isolation

The persisted runner and post acknowledgement protocol are shared infrastructure. Adapters may
compose reusable cursor, tag, pagination, and media-description helpers, but one provider's headers,
authentication transform, response parser, or endpoint behavior must not affect another provider.
There is no Python extractor, provider archive database, or sidecar execution path in production.
