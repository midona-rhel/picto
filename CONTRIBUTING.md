# Contributing to Picto

Work is tracked as GitHub issues and lands through reviewed pull requests.
`main` contains released code only; development happens on `dev`.

```
feature/… bug/… ──PR (squash)──▶ dev ──cut──▶ release/<version> ──PR (merge)──▶ main ──tag v<version>
                                  ▲                                                │
                                  └──────────────── PR main → dev (merge) ─────────┘
```

## 1. Start from an issue

Every change starts as an issue, created from a template:

- **Bug report** — for anyone who hit a problem.
- **Work item** — for anything you intend to implement: problem, evidence,
  test card, implementation boundary, acceptance criteria.

Give each issue one `priority:` label (P2 is the default), one `type:` label
and one `area:` label. An issue without evidence stays `status: triage`.
Search for related issues first and link them rather than restating them.

## 2. Branch from `dev`

Name branches by purpose and issue number:
`feature/123-tag-aliases`, `bug/140-wal-growth`, `perf/…`, `refactor/…`,
`ui/…`, `docs/…`, `test/…`, `chore/…`, `deps/…`. CI rejects other prefixes.

Keep one issue per branch. Inspect the working tree before editing and stage
only issue-owned changes.

## 3. Open a pull request into `dev`

- Fill in the template; start with `Closes #<issue>`.
- CI runs `verify` (renderer typecheck/tests, localization, command parity,
  Rust clippy `-D warnings` and tests). It must be green before merge.
- Every pull request gets an automated review (`code-review` workflow)
  against the review guidelines in [AGENTS.md](AGENTS.md); mention `@claude`
  in a comment to ask it a follow-up. It advises; a person still approves.
- Review the actual diff before asking for review. For a refactor, report net
  production lines and name the superseded code you deleted.
- Schema changes need a migration with backup, a reopen test, and the
  `schema change` label.
- Merge with **squash**: one commit per issue on `dev`, titled by the PR.
- Do not merge before review. Resolve every review thread first.

## 4. Release

Releases are cut from an accepted `dev` commit to `release/<version>`, gated,
merged into `main` with a merge commit, tagged, and merged back into `dev`.
The full procedure and its gates live in
[docs/RELEASE_READINESS.md](docs/RELEASE_READINESS.md). Hotfixes cut
`release/<version>` from `main` and follow the same steps.

## Engineering rules

- **One owner per fact.** Before adding a table, cache, queue, status string,
  helper or document, find the existing owner and extend it. A second copy of
  the same fact needs a written reason in the PR.
- **Delete what you supersede** in the same PR. No parallel old and new paths
  behind flags.
- **Tests protect user-visible behavior and data-safety invariants**, not
  internal mechanisms. See the testing section of
  [docs/RESIDENT_BROWSING_MODEL.md](docs/RESIDENT_BROWSING_MODEL.md).
- **Durable concepts get one normative doc home**; others link to it.
  `docs/architecture.md` describes the shipped system, not plans.

One-time repository setup (branches, rulesets, labels):
`scripts/github/setup-repository.sh`.
