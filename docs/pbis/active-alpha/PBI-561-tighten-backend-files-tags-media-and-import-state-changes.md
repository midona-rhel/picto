# PBI-561: Tighten Backend Files, Tags, Media, And Import State Changes

## AI-Generated Caveat
This PBI is AI-generated from the current backend event emitters and should be treated as a targeted audit-and-finish slice. The engineer should look for any additional coarse emitters in the same domain while implementing it.

## Priority
P0

## Problem
Files, tags, media metadata, deferred derivatives, and imports are central to visible UI behavior. If these state changes are vague, the frontend either over-refreshes or re-derives backend truth badly.

## Goal
Make these backend domains emit exact, self-describing committed deltas.

## Atomicity Rule
This PBI should finish this backend domain cluster only. Do not mix in subscriptions/folders/smart-folder detail unless a discovered emitter truly forces it.

## Scope
- [core/src/dispatch/typed/media_lifecycle.rs](./core/src/dispatch/typed/media_lifecycle.rs)
- [core/src/dispatch/typed/media_io.rs](./core/src/dispatch/typed/media_io.rs)
- [core/src/dispatch/typed/media_metadata.rs](./core/src/dispatch/typed/media_metadata.rs)
- [core/src/dispatch/typed/tags.rs](./core/src/dispatch/typed/tags.rs)
- [core/src/dispatch/typed/selection.rs](./core/src/dispatch/typed/selection.rs)
- [core/src/import/service.rs](./core/src/import/service.rs)
- [core/tests/events_contract.rs](./core/tests/events_contract.rs)

## Required Outcome

### Files / lifecycle
- exact affected hashes
- exact status/lifecycle consequences
- exact scope consequences where obvious

### Media metadata
- exact changed fields such as:
  - name
  - rating
  - notes
  - source URLs

### Deferred derivatives
- exact derivative fields such as:
  - thumbnail
  - dominant color
  - dominant color hex
  - phash
  - analysis/enrichment

### Tags
- exact hashes changed
- exact tags added/removed
- exact direct consequences such as `untagged`
- smart-folder scope consequences where relevant

### Import
- one final combined delta per completed import action

## Look For Adjacent Improvements
- collapse duplicate change-impact assembly in this slice
- remove no-op emits
- tighten selection-based emitters if they still omit resolved hashes
- collapse near-duplicate backend commands or emit helpers where the only difference is small input-shape variation

## Acceptance Criteria
1. These emitters no longer rely on vague domain-only changes.
2. Deferred derivatives emit authoritative state changes.
3. Import paths emit final combined deltas rather than noisy partial ones.

## Validation
- backend contract tests for each emitter family
- manual audit of emitted payload shapes in representative flows
