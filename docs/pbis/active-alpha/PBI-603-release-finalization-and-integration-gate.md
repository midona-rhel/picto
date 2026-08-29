# PBI-603: Release finalization and integration gate

## Priority
P0

## Current evidence

- The worktree has no unresolved merge entries and `git diff --check` passes.
- `npm run alpha:verify` passes: TypeScript, 657 Vitest tests, 454 Rust tests, command parity, and
  source release audit are green without warning spam.
- Gallery-dl and OnlyFans sidecars build from pinned sources and pass packaged self-tests.
- The artifact audit, macOS ZIP/DMG assembly, and packaged fresh-library startup/settlement smoke
  pass. The alpha is intentionally ad-hoc signed and not notarized.
- The remaining integration work is to commit the reviewed release slices and complete the short
  packaged behavior smoke before deleting this gate.

## Scope

This ticket closes integration debt only. It must not introduce another architecture layer.

1. Remove test warning noise without suppressing React diagnostics.
2. Port tests that still assert live behavior to current APIs.
3. Delete tests that only exercise quarantined legacy code.
4. Delete or demote tests that only pass preconfigured values through mocked layers; they may remain
   focused unit coverage, but cannot be cited as evidence that a user workflow works.
5. Give current frontend tests one explicit browser-like test environment.
6. Keep the pre-release schema canonical; Picto has no released database versions to migrate.
7. Delete dead duplicate persistence modules and other code proven to have no caller.
8. Keep future commits and refs on the verified scrubbed lineage; do not reintroduce backup refs or
   copied audit artifacts.
9. Run the packaged app smoke scenarios after the automated gate is green.

## Acceptance criteria

- `git diff --name-only --diff-filter=U` is empty.
- `git diff --check` passes.
- `npm run alpha:verify` passes without excluding current code.
- Current frontend tests run in the release verification lane without warning spam.
- Fresh schema creation and restart recovery pass without pre-release migration scaffolding.
- App smoke passes for import, grid, inspector, folder, flattened multi-file subscription import,
  duplicate manager, tag manager, and AI-tagging entry points.
- Public branch history contains no personal absolute paths, reference-product names, copied source,
  generated captures, credentials, databases, or personal media.
- No active PBI claims completion while its required gate is red.
