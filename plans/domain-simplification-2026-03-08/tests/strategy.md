# Test Strategy Simplification Plan

## Actual Goal

The goal is not to maximize the raw number of tests. The goal is to maximize confidence per line of test code.

Right now the project has too many narrow tests proving internal implementation details and not enough workflow coverage.

## Hard Truths

1. Fifteen tests for one tag-editing edge case is usually a smell, not quality.
2. A file-size guard test is not product confidence.
3. Registry-shape tests are often a weak substitute for interaction tests.
4. If a test breaks every time you move code without changing behavior, it is probably overfit.

## Keep

1. pure-function tests with real algorithmic value
   - scope invalidation
   - layout math
   - image drag state machines
   - media QoS scheduling
2. backend orchestration tests for runtime, import, gallery-dl, ffmpeg

## Reduce

1. tests that only mirror implementation splits
2. tests that enforce file sizes or layering opinions
3. multiple menu-registry tests that can be covered by one interaction workflow
4. tiny store setter tests

## Add

1. frontend workflow tests by user journey
   - import file, view file, edit metadata, delete file
   - create folder, move file into folder, reorder folder items
   - create smart folder, navigate to it, edit it, delete it
   - create tag, add tag to file, remove tag, merge tag
   - create flow, add subscription, run it, see progress, stop it
   - review duplicate and resolve it
2. backend integration workflows by command path
   - import pipeline
   - folder membership and reorder
   - smart-folder query
   - subscription run
   - PTR bootstrap and sync

## Structural Rules

1. Prefer one integration test over many micro-tests when the behavior is user-visible.
2. Keep pure-function tests only where the function is genuinely tricky.
3. Organize tests by workflow, not by implementation fragment.
4. For frontend, put high-value workflows under a predictable integration area rather than scattering them through many `__tests__` directories.

## Suggested Test Layout

1. `src/test/integration/grid/`
2. `src/test/integration/sidebar/`
3. `src/test/integration/tags/`
4. `src/test/integration/subscriptions/`
5. `src/test/integration/viewer/`
6. `core/tests/workflows/` or a similar grouped integration area

## Immediate Audit Targets

1. `src/features/grid/__tests__/imageGridSizeGuard.test.ts` should probably be deleted.
2. menu-registry and action-registry tests should be reduced to a smaller number of behavior-level tests.
3. tag and subscription flows need more workflow coverage and fewer implementation-detail assertions.

## Definition Of Better

1. fewer tests overall
2. more user-journey coverage
3. less breakage from harmless refactors
4. higher confidence when changing real behavior
