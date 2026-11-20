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

## Write so that a person can understand it

People read everything you write here: code, comments, documentation, commit
messages, pull requests, and what you say to the user. Someone who was not
part of the work must be able to read it once and know exactly what you mean.
Writing that leaves the reader guessing is a defect, the same as a bug.

- Say the literal thing: what it is, where it is, and what it does. "Show the
  similarity as small grey text to the right of the counter" can be built and
  checked. "The similarity, kept quiet" cannot; the reader has to guess what
  you meant.
- Use plain words. No metaphor, no euphemism, and no word chosen because it
  sounds good. If you would not say the sentence out loud to a colleague,
  rewrite it.
- Write whole sentences. Do not shrink an idea into a fragment or a label and
  leave the reader to work out the rest.
- Be specific. Give the file, the control, the size, the colour, the number.
  "Refined", "cleaner" and "improved" say nothing on their own.
- Use the names the user and the code already use. Do not make up a new term
  for something that already has a name.
- The same applies to code. A name says what the thing is. A comment says why
  the code is there, in a plain sentence. Do not write code or comments that a
  reader has to decode.
- Read what you wrote before you send or commit it, as someone who was not
  there. If a sentence can be read two ways, or you are not sure a reader
  would understand it, rewrite it until you are sure.

### Every file you modify

Whenever you modify a file, you must also do this in the same change:

1. Read all the comments and documentation in that file, not only the lines
   next to your change.
2. Rewrite every comment and every piece of documentation in it that fails
   the standard above, so that the whole file passes when you are done.
3. Do the same for the documentation that describes what you changed, such as
   its paragraph in a README or in `docs/`.

This applies every time, to you and to every agent you start, and it is how
the existing text is brought up to the standard: one modified file at a time.
You do not have to do it for a file you only read.

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
- Names, comments, documentation and messages can be understood on one
  reading by someone who was not part of the work; flag any that cannot.
