# Agent rules

These rules are for anyone who changes this project, people and AI agents alike. [CONTRIBUTING.md](CONTRIBUTING.md) says how work is tracked, branched, reviewed and released. This file says how to write and what to check before you hand work in.

## Write so a person can understand it

People read everything written for this project: code, comments, documentation, commit messages, issues, pull requests and release notes. Someone who has never opened the code and was not part of the work must be able to read it once and know exactly what is meant. Writing that leaves the reader guessing is a defect, the same as a bug.

- Say the idea first, in one or two plain sentences, the way you would say it to someone. Details come after.
- Say the literal thing: what it is, where it is, and what it does. Say where things are relative to something the reader can see, such as a window, a tab or a folder.
- Write whole sentences that join related facts. Do not shrink an idea into a fragment or a label.
- Use plain words. No metaphor, no slogan, no word chosen because it sounds good, and nothing written to impress.
- Use the names the program shows and the code already uses. Do not make up a new term for something that has a name. If you must use a word only the code knows, say in the same sentence what it means.
- Be specific. Give the file, the control, the number. Words like "improved" or "cleaner" say nothing on their own.
- Describe how things are now. Leave out how they used to be and how the work went.
- Leave out what the reader does not need, including changes nobody would notice.

Read what you wrote before you send or commit it, as someone who was not there. If a sentence can be read two ways, or reads like notes someone wrote for themselves, rewrite it.

## Nothing personal

Nothing written for this project may identify a person or show what is on their computer.

- No names of people, of their accounts, or of characters or files that belong to them.
- No names of people who reported something, and no quotes from chats or messages. Describe the problem instead.
- No local paths that contain a user name, no e-mail addresses, no passwords, keys, tokens or cookies.
- Screenshots show only the part of the program being described, with nothing personal in them.

This applies to code, tests, test data, comments, documentation, commit messages, issues, pull requests and release notes. Check every text for it before you post or commit it.

## Every file you change

When you change a file, bring the whole file up to these rules in the same change, not only the lines you came for:

- rewrite comments and documentation in it that are wrong, out of date or hard to read;
- remove checks that have no reason to exist and code that nothing uses;
- fix names that say the wrong thing;
- update the documentation that describes what you changed.

## Before you hand work in

- Say which checks you ran and what they showed. A passing build is not proof that something works. Say what was tried in the running program, what was only read in the code, and what was not tried.
- Do not merge a pull request, push a tag or publish a release unless the maintainer asked for it.

## Project rules

- Before writing code, find the code that already owns the job and extend it: the `published_write` path for library changes, the existing work queues for background jobs, and `NativeSourceAdapter` for sources.
- Explain each thing in its one home: `docs/architecture.md` for how Picto works, `docs/RELEASE_READINESS.md` for releases, and `docs/user-guide` for people who use Picto.
- Never commit library data, cookies, benchmark output or build artifacts.
- Picto runs on Windows, macOS and Linux. A change must work on all three.

## Review guidelines

Reviews of pull requests check these first:

- The change does what the pull request says, and nothing it does not say.
- No second copy of something the project already does. Code that the change replaces is deleted in the same change.
- Checks guard against things that can really happen. No fallbacks for values that are always there.
- Tests protect behaviour people depend on, not the way the code happens to be written.
- Names, comments, documentation and messages can be understood on one reading by someone who was not part of the work.
- Nothing personal, as described above.
- Every library write goes through the `published_write` path, and the published change names everything it changed. A change that updates SQLite but not the library held in memory, or the reverse, is a defect, and so is a cache keyed on something the change does not bump.
- Undo restores exactly what the operation changed. Every change has an undo entry, and that entry covers every root, tag, folder and order it touched.
- Smart folders, counts, Trash and selections stay correct after the change. Selections that span a whole query, such as select-all and ranges, are resolved by the backend, never from the items the app has loaded.
- Work on a library of 1,000,000 items, with tags on 200,000 of them, stays bounded: no full scans and no SQL per item inside a loop on paths a person waits on. Ask for a `library_ops_benchmark` row when such a path changes.
- Behaviour holds on Windows, macOS and Linux: paths, file locking, upper and lower case in names, and no reliance on one system's file cache or memory behaviour.
- A schema change has a migration with a backup and a test that reopens the library.
