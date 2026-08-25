# PBI-603: Release finalization and integration gate

## Priority
P0

## Current evidence

- The worktree has no unresolved merge entries and `git diff --check` passes.
- Vitest passes 130 files and 596 tests, but asynchronous React warning noise remains.
- The source release audit passes, including repository hygiene, license declarations, sidecar
  pins, platform targets, and package configuration.
- Gallery-dl and OnlyFans sidecars use pinned source revisions; the OnlyFans transitive lock now
  resolves on every supported target.
- The public remote already contains personal absolute paths and competitor references in reachable
  documentation history. The current tree is clean, but the public history still requires a
  coordinated rewrite.
- The branch is 139 commits ahead of `origin/main` and has a large dirty integration surface,
  including separately owned Cloud Sync work.

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
8. Rewrite public history from a verified mirror to remove personal paths, competitor references,
   copied audit material, and generated captures, while preserving the explicitly approved account
   identity.
9. Run the packaged app smoke scenarios after the automated gate is green.

## Acceptance criteria

- `git diff --name-only --diff-filter=U` is empty.
- `git diff --check` passes.
- `npm run alpha:verify` passes without excluding current code.
- Current frontend tests run in the release verification lane without warning spam.
- Fresh schema creation and restart recovery pass without pre-release migration scaffolding.
- App smoke passes for import, grid, inspector, folder, flattened multi-file subscription import,
  duplicate manager, tag manager, and AI-tagging entry points.
- Public branch history contains no personal absolute paths, competitor references, copied source,
  generated captures, credentials, databases, or personal media.
- No active PBI claims completion while its required gate is red.
