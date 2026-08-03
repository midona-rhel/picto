# PBI-603: Release finalization and integration gate

## Priority
P0

## Current evidence

- The worktree has unresolved merge entries.
- Full `cargo test` passes: 153 library tests and 47 integration tests pass; one live-network
  subscription test is intentionally ignored.
- Vitest passes all 10 current suites and 40 tests. Legacy frontend tests are excluded.
- TypeScript and command parity pass.
- The Rust dispatch surface has been reduced from 164 commands to 124. Only six intentional
  Rust-only transport commands remain, all documented in the parity allowlist.
- `git diff --check` is still blocked by conflict markers and unrelated generated whitespace in
  the unresolved worktree.
- Canonical schema additions are still reconciled with open-time column checks and `ALTER
  TABLE` statements instead of ordered schema versions.

## Scope

This ticket closes integration debt only. It must not introduce another architecture layer.

1. Resolve every unmerged path and remove all conflict markers.
2. Port tests that still assert live behavior to current APIs.
3. Delete tests that only exercise quarantined legacy code.
4. Give current frontend tests one explicit browser-like test environment.
5. Convert canonical schema changes after version 100 into ordered migrations.
6. Remove the corresponding open-time compatibility checks after migration coverage exists.
7. Delete dead duplicate persistence modules and other code proven to have no caller.
8. Run the packaged app smoke scenarios after the automated gate is green.

## Acceptance criteria

- `git diff --name-only --diff-filter=U` is empty.
- `git diff --check` passes.
- `npm run alpha:verify` passes without excluding current code.
- Legacy frontend tests do not run in the release verification lane.
- Fresh schema creation and upgrades from the last released schema both pass.
- Opening a current database performs no opportunistic schema mutation.
- App smoke passes for import, grid, inspector, folder, collection, subscription navigation,
  duplicate manager, tag manager, and AI-tagging entry points.
- No active PBI claims completion while its required gate is red.
