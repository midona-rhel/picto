# PBI-519: Frontend and IPC test coverage expansion

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The coverage assessment is based on counting test files relative to source files. The actual risk from missing tests depends on how frequently the untested code changes and how critical the untested paths are. Some of these gaps may be intentionally untested (e.g., Electron main process is hard to unit test). Human review should prioritize which gaps matter most.

## Priority
P2

## Problem
The frontend has 15 test files for approximately 42,000 lines of code across 420 files. While the existing tests cover important areas (grid selection, layout math, runtime sync, tag parsing, drag-drop), several critical paths have no test coverage:

### Missing coverage areas
1. **IPC integration**: No tests verify that frontend `invokeTyped()` calls match backend dispatch commands with correct argument shapes. The `check:command-parity` script validates command names exist but not argument/response shapes.
2. **Electron main process**: No tests for `windowManager.mjs` (window creation, state persistence, display bounds validation), `media.mjs` (protocol handler, hash validation, range requests), or `registerHandlers.mjs` (IPC handler validation).
3. **State store edge cases**: Zustand stores have no direct unit tests. `runtimeSyncStore` receipt batching, `domainStore` debounced refresh recovery, and `gridMetadataStore` LRU eviction are tested indirectly at best.
4. **Coverage reporting**: vitest is not configured to report coverage metrics (`coverage` section missing from vitest.config.ts).

### What IS covered (for context)
- Grid: marquee selection, selection logic, layout math, scope model, viewer session, canvas visibility
- Sidebar: drag-drop integration
- Viewer: preload plan, navigator math
- Runtime: resource invalidator, sync workflow
- Shared: image drag, tag parsing, scroll state, thumbnail pipeline

## Scope
- Add vitest coverage reporting
- Identify and write tests for the highest-risk untested paths
- Do NOT aim for 100% coverage — focus on paths where bugs would be most costly

## Implementation
1. **Add coverage reporting**: Add `@vitest/coverage-v8` to devDependencies. Configure in `vitest.config.ts`:
   ```ts
   coverage: { provider: 'v8', reporter: ['text', 'lcov'], include: ['src/**'] }
   ```
2. **Zustand store tests** (highest value): Write unit tests for:
   - `runtimeSyncStore`: receipt batching (50ms flush), task linger timers, watchdog poll
   - `gridMetadataStore`: LRU eviction at 5,000 entries, refresh sequence tracking
   - `navigationStore`: history push/pop, back/forward, scroll restore
3. **IPC shape tests**: Write tests that verify `invokeTyped()` argument types match the generated TypeScript types from ts-rs. This catches shape drift between Rust and TypeScript.
4. **Protocol handler tests** (if feasible): Unit test `media.mjs` hash validation, MIME detection, and range request parsing in isolation.

## Acceptance Criteria
1. `npm test` runs with coverage reporting enabled.
2. At least 3 new test files added for Zustand stores.
3. Coverage report shows which areas are tested vs untested.
4. No regressions in existing tests.

## Test Cases
1. `npm test -- --coverage` produces a coverage report without errors.
2. New store tests pass and cover edge cases (LRU eviction, receipt batching, history overflow).
3. `npm run gate:alpha` still passes.

## Risk
Low. Adding tests is non-destructive. The main risk is spending time on low-value tests. The "focus on highest-risk paths" guidance mitigates this.
