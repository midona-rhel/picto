# PBI-618: Native OnlyFans Coverage

## Observed Gap

The native adapter and fixtures cover purchases, messages, timeline, archived, and pinned creator
partitions, but those partitions have not completed an authenticated production-path smoke. Clear
HLS and DASH media are supported; Widevine-protected media still stops at the missing CDM/device
boundary instead of being downloaded.

## Required Behavior

- A creator query walks purchases, direct messages, timeline, archived, and pinned posts using
  separate durable cursors under one query.
- The same source media identity encountered in multiple partitions downloads once and preserves
  every source provenance record.
- Direct image, video, audio, and clear segmented media use the shared native download/settlement
  path. Widevine media requires a provisioned CDM/device path. Inaccessible media produces a post
  warning or skipped outcome, never a false success.
- Existing Picto-managed authentication remains unchanged.

## Acceptance

1. An authenticated smoke proves purchases, messages, timeline, archived, and pinned partitions
   each produce media when the account exposes them.
2. A mixed creator run settles partitions deterministically without losing or duplicating media.
3. Reset clears all five partition cursors and reruns without hanging.
4. No OF-Scraper process, database, package, or compatibility patch is used.
