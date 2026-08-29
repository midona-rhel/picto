# Subscription Execution Contract

All subscription providers implement the same ordered query pipeline. Provider adapters normalize
authentication, pagination, post metadata, and media URLs; they do not define different progress or
settlement semantics.

## Post Boundary

For each query, Picto performs these steps in order:

1. Request one source post.
2. Record the post as traversed as soon as its metadata is available.
3. Determine whether the post contains usable media.
4. If it has no usable media, advance to the next post without incrementing posts added.
5. Download every usable media file for the current post. Files within this post may download
   concurrently.
6. Persist the downloaded bytes and ingest the post as one standalone root or collection.
7. After canonical ingestion succeeds, increment posts added.
8. Only then request the next source post.

A provider must not discover a large source window and download it later as a bulk phase. It must
not download or ingest media belonging to a later post while the current post is unsettled.

## Limits And Progress

`Posts per run` is the maximum number of successfully ingested source posts for each query in one
run. Traversed posts without usable media, inaccessible posts, archive hits, and failed posts do not
consume this budget.

The persisted counters mean:

- `Posts traversed`: source posts whose metadata Picto inspected.
- `Posts added`: source posts whose complete usable media set reached canonical library state.
- `Files downloaded`: usable media files whose bytes were downloaded and persisted.

Progress is published while each current post downloads and ingests. A run cannot report a post as
added before canonical ingestion succeeds. Retry and restart resume from the last settled post;
source identity keeps repeated work idempotent.

## Provider Requirements

- Gallery providers pause extraction at each post boundary until Picto acknowledges settlement.
- Gallery imports treat the entire gallery as the one current post.
- Query providers continue past posts without usable media until the added-post budget is reached or
  source history ends.
- A provider-specific downloader may retain its own cache, but that cache cannot redefine Picto's
  ordering, counters, or completion boundary.
