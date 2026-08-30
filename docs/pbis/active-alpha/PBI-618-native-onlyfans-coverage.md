# PBI-618: Native OnlyFans Coverage

## Observed Gap

The current OnlyFans bridge does not reliably publish all creator purchases and message media, and
its behavior depends on OF-Scraper's internal database and process lifecycle.

## Required Behavior

- A creator query walks purchased posts, direct-message media, and the creator feed using separate
  durable cursors under one query.
- The same source media identity encountered in multiple partitions downloads once and preserves
  every source provenance record.
- Accessible image, video, audio, and DRM-backed media use the shared native download/settlement
  path. Inaccessible media produces a post warning or skipped outcome, never a false success.
- Existing Picto-managed authentication remains unchanged.

## Acceptance

1. Fixtures and an authenticated smoke prove purchases, messages, and feed each produce media.
2. A mixed creator run settles partitions deterministically without losing or duplicating media.
3. Reset clears all three partition cursors and reruns without hanging.
4. No OF-Scraper process, database, package, or compatibility patch is used.
