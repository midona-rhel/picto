# Picto release readiness

Picto ships for these platforms only:

- macOS on Apple Silicon (`arm64`), distributed as DMG and ZIP artifacts;
- Windows on x86-64, distributed as an NSIS installer;
- Linux on x86-64, distributed as an AppImage.

Intel macOS is not a supported build or runtime target. Package configuration, CI artifact names,
native addons, and downloaded native tools must not imply otherwise.

## Required gates

1. `npm run release:audit` passes before compilation.
2. TypeScript, focused UI tests, the production frontend build, Rust tests, and native-addon build
   pass on every supported runner.
3. `npm run release:audit -- --artifacts` passes after native binaries are built.
4. The packaged smoke test opens the app with an isolated library, loads and settles the renderer,
   plays a VP9 WebM through Picto's media protocol, closes the native library, and removes its
   temporary data without native-module errors. Import, subscription, and thumbnail behavior remains covered by
   focused integration tests rather than being overstated as part of this smoke test.
5. Cloud Sync (experimental) passes its separately owned persistence, packaged-build, and
   restart-recovery checks.

## Repository hygiene

- Do not commit generated binaries, databases, credentials, local settings, screenshots, audit
  captures, build caches, or extracted third-party application source.
- Product and test assets live under an explicit asset or fixture directory and carry provenance
  and license information when Picto did not create them.
- Third-party runtimes are fetched from pinned revisions or release URLs, verified where publisher
  digests exist, and accompanied by version-exact notices.
- Release commits must contain no personal absolute paths, private-key markers, or reference-source
  references. Run the source audit before publishing history.

## Publication gates

- The accepted release icon is wired for macOS, Windows, and Linux. macOS packages consume the
  generated native `Picto.icon` layer bundle on an Apple Silicon macOS 26 runner; Apple `actool`
  compiles its Liquid Glass asset catalog and legacy ICNS representation during packaging.
- Public Apple Developer ID signing and notarization are not part of the alpha gate. The macOS
  alpha packages are signed with the project's private self-signed certificate so packaged
  binaries have one stable identity.
- Pull requests to `release/**` branches run verification. Release-branch pushes run verification
  and all three platform package/smoke jobs in parallel; manual runs can also exercise them. Tags
  only publish the exact artifacts from a successful release-branch push for the same commit; they
  never rebuild. Branch and pull-request rules are in [CONTRIBUTING](../CONTRIBUTING.md).
- Every subscription provider is implemented by the native Rust source crate. Release packages
  contain no Python runtime, gallery-dl, OF-Scraper, bridge script, or provider-owned history store.
- TypeScript, the production build, the complete frontend and Rust test suites, command parity,
  strict Clippy, and source/artifact audits pass. React scheduling warnings are clean.
- `npm audit --omit=dev` reports no known production dependency vulnerabilities. The source license
  and repository-hygiene audit passes; packaged artifact licenses remain part of the platform gate.
- The release candidate is separated into reviewed commits and the worktree is clean; generated
  build output remains untracked.
- The public branch and tags have been rewritten to remove audited personal absolute paths,
  reference-product names, copied audit material, and generated captures. Keep the local lineage on
  that scrubbed base before publishing further work so removed objects are not reintroduced.

## Release procedure

### Prepare locally, then review

Preparation and publication are separate actions. When asked to prepare a release, make the local
commits and notes, report the outstanding gates, and stop for review. Do not push a release branch
or tag until authorized: the branch starts packaging, and the tag starts publication.

Use the Git CLI to inspect `status`, `log`, and remote refs before choosing the next version.
`git ls-remote --heads --tags origin` can verify refs even if GitHub CLI authentication is unavailable.
Do not infer the remote state from local tags alone. Preserve unrelated work and never stage an
entire worktree blindly; exclude synthetic libraries, benchmark output and local investigation files.

Implementation lands on `dev` through reviewed pull requests ([CONTRIBUTING](../CONTRIBUTING.md)).
`main` holds released code only. Release preparation happens on `release/<version>`, cut from the
accepted `dev` commit (or from `main` for a hotfix). Never rewrite published history to prepare a release.

Synchronize the application version in all of these files without updating unrelated dependencies:

- `package.json` and both root-package version entries in `package-lock.json`;
- `native/picto-node/package.json` and both root-package entries in its `package-lock.json`;
- `core/Cargo.toml`, `native/picto-node/Cargo.toml`, and `picto-sources/Cargo.toml`;
- the corresponding three application package entries in `Cargo.lock`.

`picto-library` has its own crate version; do not bump it merely to match the desktop release.
The pinned Electron runtime version is also independent of the Picto application version.

Create `docs/<version>-release-notes.md` with a `# Picto <version>` heading and short `* ` bullets,
matching previous releases. Describe user-visible changes, not implementation details, issue IDs,
test counts or development history. Mention material upgrade/startup tradeoffs plainly. Preserve
older version notes; replace a temporary next-release draft with the versioned file. This one file
is bundled for offline What's New and used as the GitHub release body.

Run the source release audit and `git diff --check` on the complete intended commit, including new
source files. Record which implementation checks already passed and which remain unverified; do
not claim a fresh full-suite pass from an earlier report. Local release-profile checks can diagnose
a Windows debug-toolchain problem, but do not waive CI's configured checks.

For changes to library opening or switching, finish the approved bounded packaged-app smoke on
synthetic data: open, switch, cancel/quit during loading, and reopen. Record process ownership,
deadlines and results. The standard alpha smoke is not a substitute for those interaction cases.
Check normal startup separately from switching: the main window currently appears after the initial
native open, so switching progress does not establish visible cold-start progress.

At the review handoff, provide the local commit IDs, proposed version, notes, remaining checks and
whether anything has been pushed. Test consolidation is not a reason to restart a performance
campaign; preserve meaningful correctness and data-safety coverage.

### Run the release gate and publish after approval

1. Cut `release/<version>` from the accepted `dev` commit and push it. Commit the synchronized
   package versions and the versioned notes on that branch. Keep issue references in commits, not
   release notes. Every push to the branch runs the Alpha Gate: renderer tests, Rust lint/tests, and
   macOS/Windows/Linux packaging and smoke together. Every job must pass on the final head.
2. After the final release-branch gate passes, open a pull request from `release/<version>` into `main`
   and merge it with a **merge commit**
   (squash is disabled for `main`), so the gated release commit stays in `main`'s ancestry.
   The required `branch-policy` check verifies the exact package/smoke artifacts before merge.
   If the PR was opened before the gate finished, rerun its CI after the gate succeeds.
3. Tag the release-branch head (not the merge commit) as `v<version>` and push the tag. Publication
   checks the matching release branch, main ancestry, successful gate commit, and all three
   package/smoke artifact sets. It downloads those package artifacts by ID with digest verification
   and attaches the bundled notes.
4. Open a pull request from `main` into `dev` and merge it with a merge commit, so release-branch
   fixes and the version bump return to development. Release branches are kept as the release record.

A hotfix follows the same steps with `release/<version>` cut from `main` instead of `dev`.

No test, lint, audit, signing requirement, or packaged smoke check is skipped to shorten the gate.
Parallel packaging can spend runner time on a candidate that fails verification, but avoids adding
verification time to the packaging critical path. Installers are built once, before tagging.

If a gate fails, fix it on the release branch through a pull request (and carry the fix to `dev`
in step 4), then wait for the new gate. Rerun failed jobs for transient runner failures. If publication fails, rerun only the
tagged publication job; do not move a published tag or rebuild different binaries under it. Expired
gate artifacts require rerunning the release-branch gate at the same commit before retrying publication.

Inspect the exact Alpha Gate run in GitHub Actions (or the GitHub CLI when authenticated). A manual
workflow run is useful for diagnosis but is not the qualifying release-branch push run required by
`scripts/ci/find-release-gate.mjs`. The tag must match the package version, the release branch must
point to that same commit, and the commit must be on main. All three package artifacts and all three
smoke reports must exist and be unexpired. Never create a separate manual release with unverified
local installers or use automatic developer-oriented notes instead of the versioned notes file.

After publication, verify the release body, versioned installers and updater manifests for every
platform. Keep the successful run and smoke-report links with the release handoff. Do not advertise
platform-specific performance unless it was measured on that platform.
