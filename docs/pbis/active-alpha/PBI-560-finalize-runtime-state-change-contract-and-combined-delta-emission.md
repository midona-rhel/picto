# PBI-560: Finalize Runtime State-Change Contract And Combined Delta Emission

## AI-Generated Caveat
This PBI is AI-generated from the current event contract and partial migration work. The concrete payload shape may change, but the required outcome must not: one final self-describing state change per completed action.

## Priority
P0

## Problem
The current `runtime/state_changed` path is better than before, but still not consistently strong enough:

- some payloads remain too coarse
- some actions still emit incomplete consequences
- some completed actions still behave like a sequence of internal changes rather than one final delta

## Goal
Make `runtime/state_changed` the single authoritative committed-change event with a self-describing payload and one final combined delta per completed action.

## Atomicity Rule
This PBI should focus on the contract and emission model itself. Detailed per-domain refinements belong in PBI-561 and PBI-562.

## Scope
- [core/src/runtime_contract/state_change.rs](./core/src/runtime_contract/state_change.rs)
- [core/src/runtime_contract/change_builder.rs](./core/src/runtime_contract/change_builder.rs)
- [core/src/events.rs](./core/src/events.rs)
- [core/tests/events_contract.rs](./core/tests/events_contract.rs)

## Required Contract Properties
1. Event name remains `runtime/state_changed`.
2. Payload describes:
   - direct changes
   - derived committed consequences
3. One completed action emits one final combined delta.
4. Progress/log/task lifecycle events remain separate.

## Required Data Quality
Payload must be able to describe, where applicable:

- affected hashes
- affected ids by domain
- changed metadata fields
- changed derivative fields
- changed scopes
- changed sidebar counts
- tag add/remove details
- membership changes

## Look For Adjacent Improvements
- simplify change-builder presets
- remove redundant preset overlap
- tighten contract naming if any implementation-led names remain
- collapse near-duplicate change-impact presets or emit helpers that describe the same committed consequence shape

## Acceptance Criteria
1. Contract tests prove one final combined state change for completed actions.
2. The event payload shape is self-describing enough for targeted frontend reconciliation.
3. Progress is not mixed into state-changed events.

## Validation
- `cargo test --manifest-path core/Cargo.toml --test events_contract --quiet`
- additional contract tests for merged deltas
