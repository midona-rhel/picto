# Test Strategy

## Purpose

Replace test sprawl with workflow coverage.

## Current Truth

- Many tests prove local wiring, wrappers, and transitional surfaces rather than core user workflows.
- This inflates line count without protecting the real product model.

## Target Truth

- Keep only:
  - parser and normalization unit tests
  - predicate compiler unit tests
  - a few DTO or utility tests
  - domain workflow integration tests

## Rename Map

- update test names and fixtures to use `media_entity`, `alias`, `implication`, and `subscription_group`

## Delete List

- Delete tests for passthrough controllers.
- Delete tests for compatibility events.
- Delete tiny component tests that duplicate one end-to-end workflow.

## DTOs and Commands Involved

- lifecycle commands
- collection commands
- tag alias and implication commands
- folder and smart-folder commands
- subscription and runtime commands

## Workflows

- import -> inbox -> active -> trash -> restore
- create collection -> add members -> hidden-member behavior
- alias and implication tagging
- folder and auto-tag behavior
- smart-folder predicate update
- subscription-group run with dedupe and throttle
- runtime snapshot + receipt-driven refresh

## Acceptance Criteria

- Total test count drops sharply while coverage of real workflows increases.
- No deleted test protected only a shim or naming alias.
- Workflow tests read like product behavior, not implementation trivia.
