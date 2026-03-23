# PBI-579: Cross-layer testing rules

## Priority
P1

## AI-generated caveat
This document is intentionally blunt. The goal is to stop test structure from drifting into a pile of large source files, scattered inline test blocks, and tiny one-off cases that do not prove real behavior.

## Problem
The project currently has too many tests that are:

- embedded directly in large production files
- shaped around implementation details instead of clear boundaries
- duplicated across nearby modules
- too small to prove the real flow
- too scattered to be easy to review or maintain

That makes the code harder to read and the tests less trustworthy.

This PBI locks one test structure for the greenfield reset set so the database, backend, media delivery, frontend boundary, and UI reset work all follow the same testing rules.

This is a prerequisite PBI for the greenfield reset set. Read and apply it before implementing the other reset PBIs in this series.

This PBI must also follow the cross-layer comment rules in [PBI-580-cross-layer-comment-rules.md](./docs/pbis/active-alpha/PBI-580-cross-layer-comment-rules.md).

## Testing goals
The test structure must make these things true:

- production files stay focused on production code
- tests sit next to the boundary they are proving, but in separate test files
- most important behavior is proven by a few clear end-to-end or boundary-level tests, not by a huge pile of micro-tests
- unit tests exist where they are useful, but do not replace real flow tests
- every reset PBI defines the tests it needs in terms of boundary, behavior, and failure cases

## Cross-layer rules

### 1. Keep tests out of large production files
Do not grow giant source files by stuffing test modules into them.

Preferred shape:
- Rust: separate `tests/` files or small adjacent test files when the crate structure genuinely requires it
- TypeScript: separate `*.test.ts` or `*.spec.ts` files

Inline tests inside production files are only acceptable for:
- very small pure helpers
- tiny parser/formatter edge cases
- cases where the language/tooling makes a separate file clearly worse

Those should be the exception, not the default.

### 2. Test boundaries, not private implementation trivia
The main test surface should follow the actual architecture:

- database write boundary
- database query boundary
- projection/compiler boundary
- backend engine boundary
- media delivery boundary
- frontend controller boundary
- frontend UI/component boundary

Do not write a large number of tests that only prove that private helper A called private helper B.

### 3. Prefer a few strong integration tests over many tiny weak tests
For the reset line, the default test shape should be:

- a few clear unit tests for small pure logic
- boundary tests for each public module surface
- integration tests for the real flow through that slice

Examples:
- database: one test that proves create/query/update behavior through the database boundary is worth more than five tiny tests for private SQL helpers
- backend: one test that proves a public engine call returns the right DTO and state change is worth more than many transport-shaped helper tests
- frontend: one controller test that proves optimistic update plus reconcile is worth more than several tiny store tests with mocked internals

### 4. Use one test file per behavior family
Do not keep throwing unrelated cases into one giant test dump.

Preferred shape:
- one file for entity view queries
- one file for entity details
- one file for folder behavior
- one file for tag behavior
- one file for asset delivery
- one file for bulk-target behavior

Group tests by behavior family and public boundary, not by where it happened to be easiest to append another case.

### 5. Every reset PBI must define three test layers
Each reset PBI in this series should define:

- boundary tests
- behavior/integration tests
- regression or failure-path tests

If a PBI only lists unit tests, it is underspecified.

### 6. Select-all must be tested as its own target mode
`select all` is not just “a lot of hashes.”

It must be tested as its own special target mode anywhere bulk behavior matters:
- backend target resolution
- controller bulk actions
- export/import/deferred work where applicable
- UI summary and confirmation behavior

### 7. Do not test legacy shapes in the new path
The greenfield reset line is not supposed to keep old structures alive through tests.

Do not keep tests that:
- prove old DTO names
- prove old transport duplication
- prove old direct path/file APIs
- prove old inline refresh behavior

When the new boundary lands, the tests should prove the new boundary directly.

## Required test shape for the reset line
Every reset PBI in the `567+` line must follow these rules:

- separate test files, not large inline test blocks in production files
- named by behavior family and boundary
- at least one boundary-level test for each main public surface introduced by that PBI
- at least one integration-style test for the main successful flow
- explicit failure-path coverage where the PBI changes error handling, recovery, selection, or long-running work

## Done definition
This PBI is satisfied for a given reset PBI only when:

- that PBI references this test-rules document
- its test section names the relevant boundary tests and integration tests
- the planned tests are grouped by behavior family instead of dumped into unrelated files
- the new path does not rely on oversized inline test modules in production files

## Manual review checklist
- Are the tests in separate files from the main production code?
- Are they grouped by real behavior families?
- Do the tests prove the public boundary instead of private helper choreography?
- Is `select all` tested as a special target mode where bulk behavior exists?
- Are failure and recovery paths covered where the PBI changes them?
- Did the implementation delete obsolete legacy-shape tests instead of carrying them forward?
