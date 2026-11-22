# Contributing to Picto

Work is tracked as GitHub issues and lands through reviewed pull requests.
Development happens on `dev`, and releases are made from `dev`. The branch
`main` is not used.

```
feature/… bug/… ──PR (squash)──▶ dev ──cut──▶ release/<version> ──PR (merge)──▶ dev ──tag v<version>
```

## 1. Start from an issue

Every change starts as an issue, created from a template:

- **Bug report** — for anyone who hit a problem.
- **Work item** — for anything you intend to implement: problem, evidence,
  test card, implementation boundary, acceptance criteria.

Give each issue one `priority:` label (P2 is the default), one `type:` label
and one `area:` label. The `area:` labels are `library` (the crate
`picto-library`), `core` (the crate `core`), `sources` (the crate
`picto-sources`) and `gpui` (the app in `app/`). An issue without evidence
stays `status: triage`.
Search for related issues first and link them rather than restating them.

## 2. Branch from `dev`

Name branches by purpose and issue number:
`feature/123-tag-aliases`, `bug/140-wal-growth`, `perf/…`, `refactor/…`,
`ui/…`, `docs/…`, `test/…`, `chore/…`, `deps/…`. CI rejects other prefixes.

Keep one issue per branch. Inspect the working tree before editing and stage
only issue-owned changes.

## 3. Open a pull request into `dev`

- Fill in the template; start with `Closes #<issue>`.
- CI runs the `verify` workflow for every pull request. It must pass before
  the merge. Its jobs are:
  - `verify:scripts`, which runs the tests of the scripts in `scripts/`;
  - `verify:rust`, which runs the source audit, Clippy with `-D warnings`
    and the tests of the crates `core`, `picto-library` and `picto-sources`;
  - `verify:app (linux)` and `verify:app (windows)`, which build and test
    the app on those two systems and save pictures of it as artifacts of
    the run.
- Pull requests are reviewed by the Codex GitHub integration against the
  review guidelines in [AGENTS.md](AGENTS.md). Enable Automatic review for
  this repository in Codex settings, including review after new pushes.
  Use `@codex review` to request another pass. No AI-provider Actions secret
  or review workflow is needed. Wait for that review and resolve its findings
  before merging; conversation resolution alone cannot enforce that it ran.
- Review the actual diff before asking for review. For a refactor, report net
  production lines and name the superseded code you deleted.
- Schema changes need a migration with backup, a reopen test, and the
  `schema change` label.
- Merge with **squash**: one commit per issue on `dev`, titled by the PR.
- A release branch is merged into `dev` with a **merge commit**, so that the
  commit whose packages are published is in `dev`.
- Do not merge before review. Resolve every review thread first.
- The app is developed on macOS, and CI has no macOS job. Before you open a
  pull request that changes the app (`app/`) or a crate that it uses
  (`core`, `picto-library`, `picto-sources`), build the app and run its
  tests on macOS. [`app/README.md`](app/README.md), section 6, says how.
- A pull request that changes the packaging (`app/package.sh`,
  `app/packaging/`, the signing scripts) also starts the `Package` workflow,
  which builds the package for each of the three systems and checks it.
- The required `branch-policy` check is a guard for trusted maintainers, not
  tamper-proof enforcement: a same-repository PR can change its Actions job.
  Zero required approvals therefore do not mechanically guarantee that a
  release was gated. Review workflow changes explicitly; stronger enforcement
  needs an independently controlled required check or organization workflow.

## 4. Release

Releases are cut from an accepted `dev` commit to `release/<version>`, gated,
merged into `dev` with a merge commit, and tagged. The full procedure and its
gates live in [docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md).

## Engineering rules

### Authorship

Picto is maintained by Midona. AI assistants are tools, not project authors
or contributors. When working on Midona's behalf, use the configured Midona
Git identity; do not add AI author identities, `Co-Authored-By` credits,
generated-by signatures, or assistant session links to commits or PRs.
Preserve genuine human contributor attribution and third-party license notices.

Before squash-merging, remove AI attribution from the final commit message,
including trailers copied from the branch's commits. Do not rewrite published
history just to change attribution. `.mailmap` normalizes existing AI identities
in Git reports without changing the underlying commits.

The shared `.claude/settings.json` disables Claude Code's automatic commit,
PR, and session attribution. Keep that configuration when using the tool.

### Implementation

- **One owner per fact.** Before adding a table, cache, queue, status string,
  helper or document, find the existing owner and extend it. A second copy of
  the same fact needs a written reason in the PR.
- **Delete what you supersede** in the same PR. No parallel old and new paths
  behind flags.
- **Tests protect user-visible behavior and data-safety invariants**, not
  internal mechanisms: a different correct implementation should still pass,
  and the expected result must not come from the code under test.
- **The payloads of the core's commands are a tested contract.** A new or
  changed command needs an example of its payload in
  `core/tests/fixtures/ipc_payloads.json`. `cargo test` sends every payload
  in that file through the core's dispatcher (`core/tests/ipc_contract.rs`).
- **Durable concepts get one normative doc home**; others link to it.
  `docs/architecture.md` describes the shipped system, not plans.

One-time repository setup (branches, rulesets, labels):
`scripts/github/setup-repository.sh`.
