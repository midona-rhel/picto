# Subscriptions

## Purpose

Define timed external ingestion through subscriptions and subscription groups.

## Current Truth

- Subscription CRUD, query cursors, scheduling, runtime progress, and gallery-dl execution are too entangled.
- UI still teaches old “flow” naming.

## Target Truth

- `Subscription` is a timed external source definition.
- `SubscriptionGroup` is a schedule and grouping boundary for subscriptions.
- Each subscription has one or more queries with resume cursor state.
- Dedupe happens before import commit.
- Import handoff always goes through the common import pipeline.

## Rename Map

- `flow` -> `subscription_group`
- `FlowInfo` -> `SubscriptionGroupInfo`
- `run_flow` -> `run_subscription_group`
- visible “Flows” UI -> “Subscription Groups”

## Delete List

- Delete duplicated run/stop/reset orchestration between controller-style layers.
- Delete UI wording and routes that keep “flow” alive after cutover.

## DTOs and Commands Involved

- `SubscriptionInfo`
- `SubscriptionQueryInfo`
- `FlowInfo` / target `SubscriptionGroupInfo`
- run, stop, reset, pause, add-query, delete-query commands

## Workflows

- Create group -> create subscription -> add query -> schedule run.
- Group run -> query cursors advance -> dedupe prevents duplicate imports.
- Stop or reset -> runtime task updates and archive/reset state stay consistent.

## Acceptance Criteria

- Product docs and UI use `SubscriptionGroup`.
- Subscription ingestion always reuses the common import pipeline.
- Query cursor semantics are documented and tested end to end.
