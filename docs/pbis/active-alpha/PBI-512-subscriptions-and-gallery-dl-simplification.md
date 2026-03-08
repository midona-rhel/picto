# PBI-512: Subscriptions and gallery-dl simplification

## Priority
P0

## Problem
Subscriptions still carry leftover `flow` semantics, adapter bleed-through, and excess renderer orchestration.

## Goal
Reduce subscriptions to the actual product behavior.

## Implementation
1. `SubscriptionGroup` is the schedule container.
2. `Subscription` is the site-specific source definition.
3. gallery-dl remains adapter code under subscriptions only.
4. Apply one global per-canonical-site throttle, default 1 second.
5. Dedupe before import commit.
6. Keep subscription API inside the subscriptions domain, not `shared/controllers`.

## Acceptance Criteria
1. No flow fiction remains in active code.
2. Subscription UI owns only presentation and local form state.
3. Adapter-specific metadata parsing does not leak into core tags or media semantics.
