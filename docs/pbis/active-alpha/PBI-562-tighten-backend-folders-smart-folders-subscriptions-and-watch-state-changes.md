# PBI-562: Tighten Backend Folders, Smart Folders, Subscriptions, And Watch State Changes

## AI-Generated Caveat
This PBI is AI-generated and likely to uncover unfinished backend action design. If a simpler backend command/result shape would materially improve the slice, the implementing engineer should take it and document the change.

## Priority
P0

## Problem
Folders, smart folders, subscriptions, and watched-folder flows still risk emitting state changes that are too coarse for targeted frontend behavior.

## Goal
Make these backend domains emit exact, combined, committed deltas with direct and derived consequences.

## Atomicity Rule
This PBI should finish this backend domain cluster only. Do not absorb files/tags/media emitters unless a single helper absolutely forces it.

## Scope
- [core/src/folders/watch.rs](./core/src/folders/watch.rs)
- [core/src/dispatch/typed/folders.rs](./core/src/dispatch/typed/folders.rs)
- [core/src/dispatch/typed/smart_folders.rs](./core/src/dispatch/typed/smart_folders.rs)
- [core/src/dispatch/typed/subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs)
- [core/src/subscriptions/sync_engine/mod.rs](./core/src/subscriptions/sync_engine/mod.rs)
- [core/src/subscriptions/sync_engine/importing.rs](./core/src/subscriptions/sync_engine/importing.rs)
- [core/tests/events_contract.rs](./core/tests/events_contract.rs)

## Required Outcome

### Folders
- exact folder ids changed
- exact member hashes changed
- exact scope/sidebar consequences

### Folder watch
- exact folder ids and hashes
- one final combined delta for completed watch-driven actions
- no split “half of the result here, half there” behavior

### Smart folders
- exact smart folder ids changed
- exact scope/tree consequences

### Subscriptions
- exact group ids changed
- exact subscription ids changed
- exact query ids changed
- exact credential/site category consequences
- exact materialized scope consequences when content/entity state changes

## Existing Partial Progress
This slice already has some improvement:

- watched-folder import and some subscription flows already batch parts of their final delta better than before

That does not close this PBI. Finish the remaining coarse paths.

## Look For Adjacent Improvements
- collapse repeated subscriptions sidebar presets
- remove redundant double-emits where a single merged impact is enough
- tighten watch/import code paths that still produce split consequences
- collapse near-duplicate backend APIs and emit helpers in this slice instead of preserving tiny variants with different names

## Acceptance Criteria
1. No remaining coarse sidebar-only emits exist for completed actions in this slice where richer consequences are available.
2. One final combined delta is emitted per completed action.
3. Contract tests cover representative folder-watch and subscription cases.

## Validation
- `cargo test --manifest-path core/Cargo.toml --test events_contract --quiet`
- manual payload inspection for representative folder-watch and subscription actions
