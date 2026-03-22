# PBI-566: Audit And Complete Backend State-Change Coverage

## AI-Generated Caveat
This PBI is AI-generated from a live repo audit. It is intentionally concrete, but the implementing engineer should assume there may still be additional state-changing producers hidden behind helper layers or background services. If a producer commits visible state and is not listed here, add it to the same audit while working.

## Priority
P0

## Problem
The backend now emits `runtime/state_changed` widely, but coverage and quality are still inconsistent:

- some domains still use coarse helper presets instead of emitting direct and derived consequences precisely
- some command families still emit generic sidebar/domain deltas when exact ids and scopes are known
- some long-running or background producers still need a strict “final combined delta” audit
- there is no single explicit audit artifact proving which state-changing commands emit what

That leaves the frontend cleaner than before, but still too dependent on broad interpretation in several backend slices.

## Goal
Audit every backend state-changing command and background producer, then tighten `runtime/state_changed` so completed actions emit one self-describing final delta with exact ids, hashes, fields, and scopes wherever those consequences are available.

## Atomicity Rule
This PBI is about backend state-change audit and coverage. Do not broaden into frontend controller cleanup except for the minimum type/export updates required by the stricter event payloads.

## Scope
- [core/src/events.rs](./core/src/events.rs)
- [core/src/runtime_contract/state_change.rs](./core/src/runtime_contract/state_change.rs)
- [core/src/runtime_contract/change_builder.rs](./core/src/runtime_contract/change_builder.rs)
- [core/src/dispatch/typed/folders.rs](./core/src/dispatch/typed/folders.rs)
- [core/src/dispatch/typed/smart_folders.rs](./core/src/dispatch/typed/smart_folders.rs)
- [core/src/dispatch/typed/subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs)
- [core/src/dispatch/typed/selection.rs](./core/src/dispatch/typed/selection.rs)
- [core/src/dispatch/typed/media_io.rs](./core/src/dispatch/typed/media_io.rs)
- [core/src/dispatch/typed/media_lifecycle.rs](./core/src/dispatch/typed/media_lifecycle.rs)
- [core/src/dispatch/typed/media_metadata.rs](./core/src/dispatch/typed/media_metadata.rs)
- [core/src/dispatch/typed/tags.rs](./core/src/dispatch/typed/tags.rs)
- [core/src/folders/watch.rs](./core/src/folders/watch.rs)
- [core/src/import/service.rs](./core/src/import/service.rs)
- [core/src/import/existing.rs](./core/src/import/existing.rs)
- [core/src/subscriptions/sync_engine/mod.rs](./core/src/subscriptions/sync_engine/mod.rs)
- [core/src/subscriptions/sync_engine/importing.rs](./core/src/subscriptions/sync_engine/importing.rs)
- [core/src/duplicates/orchestrator.rs](./core/src/duplicates/orchestrator.rs)
- [core/tests/events_contract.rs](./core/tests/events_contract.rs)

## Known Audit Failures / Suspect Areas

### Subscriptions are still too coarse
- [subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs#L180)
- [subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs#L317)
- [subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs#L366)
- [subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs#L492)
- [subscriptions.rs](./core/src/dispatch/typed/subscriptions.rs#L630)
  - many commands still emit `ChangeImpact::subscriptions_sidebar()` plus one id list
  - the audit must decide what the true changed consequences are for groups, subscriptions, queries, schedules, credentials, and run/reset flows

### Smart folders are still mostly “sidebar + id”
- [smart_folders.rs](./core/src/dispatch/typed/smart_folders.rs#L89)
- [smart_folders.rs](./core/src/dispatch/typed/smart_folders.rs#L170)
- [smart_folders.rs](./core/src/dispatch/typed/smart_folders.rs#L224)
  - create/move/reorder still look too coarse for a self-contained state-change contract

### Folders and watch config still have sidebar-heavy emits
- [folders.rs](./core/src/dispatch/typed/folders.rs#L274)
- [folders.rs](./core/src/dispatch/typed/folders.rs#L296)
- [folders.rs](./core/src/dispatch/typed/folders.rs#L376)
- [folders.rs](./core/src/dispatch/typed/folders.rs#L394)
  - create/move/update/watch-config/clear-watch-config still depend heavily on generic `sidebar(...)`

### Helper presets need tightening
- [change_builder.rs](./core/src/runtime_contract/change_builder.rs#L404)
- [change_builder.rs](./core/src/runtime_contract/change_builder.rs#L408)
  - `sidebar(...)` and `subscriptions_sidebar()` are too easy to overuse
  - the audit should either narrow their use or replace them with richer helpers

### Completed-action audit must be explicit
- every state-changing command should be classified as:
  - query only
  - state change
  - task/progress only
  - deferred state-change producer
- that classification should live in code or adjacent audit notes, not just in someone’s head

## Required Outcome
- every completed state-changing backend action emits one final `runtime/state_changed`
- emitted deltas identify exact changed ids, hashes, fields, and scopes wherever the backend knows them
- coarse domain/sidebar presets are reduced to the cases where no richer consequence set actually exists
- background producers like watch import, deferred media work, import flows, and subscription import all follow the same final-delta rule

## Look For Adjacent Improvements
- collapse repeated change-builder presets that only differ by tiny naming
- remove double-emits where one merged impact is enough
- rename helper constructors so they describe the actual changed state instead of the implementation detail
- add missing event-contract tests whenever a new richer field is introduced
- collapse near-duplicate command flows that only exist because two code paths evolved separately

## Non-Goals
- frontend controller simplification outside the minimum payload/type updates required here
- UI rendering fixes unrelated to backend state-change coverage

## Acceptance Criteria
1. Every state-changing command and background producer in scope is explicitly audited and classified.
2. No obviously coarse `subscriptions_sidebar()` / generic `sidebar(...)` emits remain where richer consequences are available.
3. Completed actions emit one final combined `runtime/state_changed` delta.
4. Contract tests cover representative cases from subscriptions, folders/watch, smart folders, and deferred/background producers.
5. The resulting event payloads are self-describing enough that the frontend does not need to guess secondary consequences in these audited slices.

## Validation
- `cargo test --manifest-path core/Cargo.toml --test events_contract --quiet`
- targeted manual payload inspection for:
  - group create/rename/delete/schedule
  - subscription create/delete/rename/pause/reset/query changes
  - smart folder create/update/move/reorder/delete
  - folder create/update/move/watch-config/clear-watch-config
  - watch import completion
  - deferred media derivative completion
