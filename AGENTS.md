# Agent rules

- Follow [CONTRIBUTING.md](CONTRIBUTING.md). Track work as GitHub issues, not
  new PBI files; `docs/pbis/` is legacy and is being migrated to issues.
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
