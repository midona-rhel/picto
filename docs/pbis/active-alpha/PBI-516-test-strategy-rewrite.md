# PBI-516: Test strategy rewrite

## Priority
P0

## Problem
The test suite spends too much effort proving wrappers, component trivia, and internal structure instead of end-to-end workflows.

## Goal
Replace low-value test volume with workflow proof.

## Keep
1. parser and normalization unit tests
2. predicate compiler unit tests
3. a few pure utility and DTO tests

## Replace With Workflow Tests
1. import into inbox -> accept active -> trash -> restore
2. create collection -> add members -> members hidden from general scopes
3. add tag -> alias -> implication -> search/grid/inspector agree
4. folder create/move/reorder/auto-tag
5. smart folder create/edit -> count/membership update
6. run subscription group with throttle and dedupe
7. runtime snapshot + mutation/task events keep UI fresh
8. sidebar counts update after lifecycle/tag/folder/collection mutations
9. PTR remains invisible to product UI

## Acceptance Criteria
1. Tests that only prove passthrough wrappers or component-local trivia are deleted.
2. Workflow coverage becomes the primary confidence layer.
