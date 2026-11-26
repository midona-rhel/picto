# Contributing to Picto

Work is tracked as GitHub issues and reaches `dev` through reviewed pull
requests. Releases are made from `dev`. The branch `main` is not used; never
open a pull request into it, and never commit to `dev` directly.

```
feature/… bug/… ──PR (squash)──▶ dev ──branch──▶ release/<version> ──PR (merge)──▶ dev ──tag v<version>
```

[AGENTS.md](AGENTS.md) adds the rules for AI agents, the writing standard
that applies to everything in the repository, and the guidelines that the
automated review checks.

## 1. Start from an issue

Every change starts as an issue made from one of the two templates. Use the
bug report template for a problem you hit. Use the work item template for
anything you intend to implement; it asks for the problem, evidence, a test
card, the implementation boundary and acceptance criteria.

Give each issue one `priority:` label (P2 unless another fits), one `type:`
label and one `area:` label. The `area:` labels are `library` (the crate
`picto-library`), `core` (the crate `core`), `sources` (the crate
`picto-sources`) and `gpui` (the app in `app/`). An issue without evidence
keeps the `status: triage` label. Search for related issues first and link
them instead of repeating them.

## 2. Branch from `dev`

Name the branch by its purpose and the issue number, for example
`feature/123-tag-aliases` or `bug/140-wal-growth`. The allowed prefixes are
`feature/`, `bug/`, `perf/`, `refactor/`, `ui/`, `docs/`, `test/`, `chore/`
and `deps/`; CI rejects any other.

Keep one issue per branch. Look at the working tree before you edit, and
stage only the changes that belong to the issue.

## 3. Open a pull request into `dev`

- Fill in the template and start it with `Closes #<issue>`.
- Read the whole diff yourself before asking for review. For a refactor,
  give the net change in production lines and name the code you deleted.
- CI runs the `verify` workflow on every pull request, and it must pass
  before the merge. Its jobs are:
  - `verify:scripts`, which runs the tests of the scripts in `scripts/`;
  - `verify:rust`, which runs the source audit, Clippy with `-D warnings`
    and the tests of the crates `core`, `picto-library` and `picto-sources`;
  - `verify:app (linux)` and `verify:app (windows)`, which build and test
    the app on those two systems and save screenshots of it as artifacts of
    the run.
- CI has no macOS job, and the app is developed on macOS. Before you open a
  pull request that changes the app (`app/`) or a crate it uses (`core`,
  `picto-library`, `picto-sources`), build the app and run its tests on
  macOS as section 6 of [app/README.md](app/README.md) describes.
- A pull request that changes the packaging (`app/package.sh`,
  `app/packaging/`, the signing scripts) also runs the `Package` workflow,
  which builds and checks the package on all three systems.
- Schema changes need a migration with a backup, a test that reopens the
  migrated library, and the `schema change` label.
- The Codex GitHub integration reviews every pull request against the review
  guidelines in [AGENTS.md](AGENTS.md). Comment `@codex review` to ask for
  another review. Wait for the review, and resolve every review thread
  before merging.
- Merge with a squash merge, so that each issue is one commit on `dev`, titled
  like the pull request. A release branch is the exception: it is merged into
  `dev` with a merge commit, so that the commit whose packages were published
  is part of `dev`.
- The required `branch-policy` check runs from the workflow files of the pull
  request itself, so a pull request can change what it checks. Review every
  change to `.github/workflows/` by reading it.

## 4. Release

A release branch `release/<version>` is made from an accepted commit on
`dev`, checked by the release gates, merged into `dev` with a merge commit,
and tagged. [docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md) gives the
full procedure and the gates.

## Engineering rules

- Before you add a table, cache, queue, status value, helper or document,
  find the existing code or document that already owns that fact and extend
  it: the `published_write` path for library changes, the existing work
  queues for background jobs, `NativeSourceAdapter` for sources. A second
  copy of the same fact needs a reason written in the pull request.
- Delete the code you replace in the same pull request. Do not keep an old
  and a new path side by side behind a flag.
- Tests protect behavior the user sees and the safety of their data, not how
  the code works inside. A different correct implementation must still pass,
  and the expected result must not be computed by the code under test.
- The payloads of the core's commands are a tested contract. A new or
  changed command needs an example payload in
  `core/tests/fixtures/ipc_payloads.json`; `cargo test` sends every payload in
  that file through the core's dispatcher (`core/tests/ipc_contract.rs`).
- Each lasting concept is documented in one place, and other documents link
  to it. Extend that place (`docs/architecture.md`,
  `docs/RELEASE_READINESS.md`, the user guide) instead of adding a document
  per module, and delete text it replaces. `docs/architecture.md` describes
  the system as it is shipped, not plans.
- Never commit library data, credentials, cookies, local absolute paths,
  benchmark output or build artifacts.

## Authorship

AI assistants are tools, not authors of the project. Commits and pull
requests use the maintainer's Git identity and carry no AI author identity,
`Co-Authored-By` line, "generated by" signature or link to an assistant
session. Before a squash merge, remove such lines from the commit message,
including ones copied from the branch's commits. Keep the attribution of real
human contributors and third-party license notices. Do not rewrite published
history to change attribution; `.mailmap` maps the AI identities in older
commits to the maintainer in Git's reports.

The shared `.claude/settings.json` turns off Claude Code's attribution in
commits, pull requests and session links. Keep it that way.

The labels, branches and rulesets of the repository are set up once with
`scripts/github/setup-repository.sh`.
