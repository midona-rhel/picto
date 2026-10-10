# Agent rules

- Follow [CONTRIBUTING.md](CONTRIBUTING.md). Track work as GitHub issues, not
  PBI files.
- Branch from `dev`, named by purpose (`feature/`, `bug/`, `perf/`,
  `refactor/`, `ui/`, `docs/`, `test/`, `chore/`, `deps/`) plus the issue
  number. Never commit to `dev` or `main` directly, and never open a pull
  request into `main` from anything but `release/<version>`.
- Open the pull request into `dev` and stop. Do not merge, push a release
  branch, or push a tag without explicit approval from the user.
- Before writing code, find the existing owner of the behavior: the
  `published_write` path for library mutations, the existing work queues for
  background jobs, `NativeSourceAdapter` for sources. Extend it; do not add a
  parallel path. Delete superseded code in the same pull request.
- Report results honestly: list the checks you actually ran, their output,
  and what remains unverified. Compilation is not runtime proof; the user
  accepts behavior in the running app with a test card (starting state,
  actions, expected result, evidence to report).
- Do not add a document per module. Extend the existing normative home
  (`docs/architecture.md`, `docs/RELEASE_READINESS.md`, the user guide) and
  delete superseded prose; Git keeps history.
- Never commit library data, credentials, cookies, local absolute paths,
  benchmark output or build artifacts.

## Review guidelines

Automated and human reviews of pull requests check these first:

- Every library write goes through the `published_write` path, and the published
  delta names everything it changed. Flag a mutation that updates SQLite but
  not the resident projection, or the reverse, and any cache keyed on
  something the change does not bump.
- Undo restores exactly what the operation changed. Flag mutations without an
  undo entry or with one that misses a touched root, tag, folder or order.
- Smart folders, counts, Trash and selections stay correct after the change;
  query-wide selections (select-all, ranges) are resolved by the backend, never
  from the renderer's loaded window.
- Work on a library of 1M items with tags on 200k stays bounded: no full scans
  or per-item SQL inside a loop on interactive paths. Ask for a
  `library_ops_benchmark` row when a hot path changes.
- Behavior must hold on Windows, macOS and Linux: paths, file locking, case
  sensitivity, and no reliance on one OS's file cache or memory behavior.
- Schema changes have a migration with backup and a reopen test.
- Tests protect user-visible behavior and data safety, not internals; flag
  tests that only restate the implementation.
