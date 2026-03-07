# PBI-330: Business Logic Contract Audit And Conformance Tests

## Priority
P0

## Audit Status (2026-03-07)
Status: **Not Implemented**

## Problem
The codebase currently allows accidental behavior to become de facto contract:
- comments can describe the wrong semantics
- two code paths can silently disagree
- no test enforces the intended business rule

This is exactly how drift happens for:
- `system:all`
- inbox visibility
- select-all semantics
- folder/tag scope matching
- untagged and uncategorized behavior

## Goal
Write explicit business-logic contracts and backend conformance tests for the core scope and mutation semantics.

If the intended product rule is not encoded in a test, it will drift again.

## Scope
- backend business rules around:
  - status scopes
  - select-all behavior
  - tag search semantics
  - folder union/exclusion semantics
  - untagged
  - uncategorized
  - recently viewed
  - mutation receipts for common actions

## Implementation
1. Create a contract doc for scope semantics and mutation semantics.
2. Add backend tests that assert intended behavior, not current accidental behavior.
3. Where implementation and intended product behavior disagree, fix the implementation first, then write the test.
4. Add a review rule: comments describing business behavior must cite the canonical contract/test or be removed.

## Acceptance Criteria
1. Core scope semantics are documented as product contracts, not inferred from implementation.
2. Backend tests exist for each major scope and select-all rule.
3. Comments cannot contradict the tested contract without failing review.
4. Known drift cases are codified and locked down.

## Minimum Contract Cases
1. `system:all` does not silently include inbox if product says active-only.
2. `select all` matches the current visible grid scope exactly.
3. `uncategorized` excludes inbox and trash if product says active-only.
4. Multi-tag search semantics are explicit and tested.
5. Multi-folder inclusion semantics are explicit and tested.

## Risk
Medium. This will force several product-rule decisions that are currently ambiguous. That is required, not optional.
