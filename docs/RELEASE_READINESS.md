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

## Acceptance integration handoff (2026-10-11)

- PR #88 passed review and CI and was merged into `dev` as `18e22a82` after user approval.
- PR #90 passed review and CI and was merged into `dev` as `747e5204`. This accepted commit is
  the base for `0.6.16-alpha`; the user's uncommitted work on `main` is excluded.
- The user smoke-tested the merged debug build. The detailed manual evidence below predates the
  final review fixes; it must not be presented as a fresh full manual pass on the release head.
- All available macOS manual rechecks passed against acceptance code commit `df9b2046`. The evidence
  index is `artifacts/acceptance/RESULTS.json`, with the final captures under
  `artifacts/acceptance/round7/`.
- The Google Drive check used a real remote round trip: all 502 uploaded, downloaded, and restored
  blobs matched by hash. Dropbox was not checked because it is not installed on the test host.
- Windows memory remains untested because the acceptance host is macOS. This is an unavailable
  platform check, not a test pass. The user accepted both Windows manual testing and Dropbox
  remote delivery as unverified exceptions for this acceptance. Automated CI and package gates
  are not waived.
- The six-filter UI check used Date Imported because the UI has no capture-date control;
  `captured_at` remains covered by runtime evidence only.
- Picto Pack reimport through the native chooser restored conflicting metadata without changing
  existing undo or redo entries. The always-rendered, hidden CloudSyncBar fallback text was not an
  active import indicator; native completion, modal closure, restored notes, and exact history state
  are recorded in `round7/pack-history-fixed.json`.

## 0.6.16-alpha local preparation

- `release/0.6.16-alpha` is based on accepted `dev` commit `747e5204`. Its additional
  changes are version synchronization, release notes and the release-only packaging fix in #92.
- Fresh `npm run alpha:verify` passed: 202 frontend/Electron test files, 1,340 tests passed and
  one skipped; 633 Rust tests passed and four ignored; source audit, localization (1,482 messages
  across eight languages), TypeScript, strict Clippy and command parity passed. Local Rust is
  1.96; the accepted dev CI also passed with its pinned toolchain.
- The initial sandboxed attempt could not bind local HTTP fixtures or access macOS network
  configuration; the complete gate passed after rerunning with those permissions. No tests were
  disabled to obtain the pass.
- `npm run build` passed, including viewer assets and generated license notices.
  `npm audit --omit=dev` reported zero known vulnerabilities. Non-failing output included React
  `act(...)` warnings in GroupSurface/ScopeTitle tests and Vite's large-chunk warning.
- The release branch was pushed at `2afd34c6`. Alpha Gate run `38154619086` passed verification,
  all three package jobs and packaged smoke tests, but log inspection found an unacceptable
  macOS ad-hoc signing and flat-icon fallback. Those artifacts are not publication candidates.
  Issue #92 makes these requirements fail closed; all three platform gates must pass again on
  the corrected release head. No tag or publication has been created.
- The first corrected gate (`38156887199`, `adeb0fc4`) passed Windows/Linux packaging and smoke,
  but macOS failed to resolve the signing identity and later timed out. The temporary keychain
  must also be on the user's search list for certificate-chain resolution, then removed from it
  during cleanup. A bounded signing preflight now checks actual key usability before packaging;
  bounded keychain operations prevent cleanup from hanging until the CI timeout.
- Gate `38159701393` proved the real signing preflight and app signature work. It exposed an
  incorrect certificate-extraction argument and a macOS admin-trust removal prompt. The verifier
  uses the joined `--extract-certificates=prefix` form, covered by a real macOS read-only test;
  hosted VM disposal now owns the public trust record while private keychains are still deleted.
- Issue reconciliation is tracked in #91; partial fixes and deferred work remain open rather than
  being closed merely because the integration merged.

## Publication gates

- The accepted release icon is wired for macOS, Windows, and Linux. macOS packages consume the
  generated native `Picto.icon` layer bundle on an Apple Silicon macOS 26 runner; Apple `actool`
  compiles its Liquid Glass asset catalog and legacy ICNS representation during packaging.
- Public Apple Developer ID signing and notarization are not part of the alpha gate. The macOS
  alpha packages are signed with the project's private self-signed certificate so packaged
  binaries have one stable identity. Packaging requires a valid signing identity, then verifies
  the final app's signature, matching leaf certificate and compiled icon catalog before archiving.
  Self-signed certificate trust is installed only on disposable GitHub-hosted macOS runners and
  expires with that VM. Private keys and temporary keychains are deleted and the original keychain
  search list is restored by the build. Local and self-hosted machines must already trust the identity.
- Pull requests to `release/**` branches run verification. Release-branch pushes run verification
  and all three platform package/smoke jobs in parallel; manual runs can also exercise them. Tags
  only publish the exact artifacts from a successful release-branch push for the same commit; they
  never rebuild. Branch and pull-request rules are in [CONTRIBUTING](../CONTRIBUTING.md).
- Every subscription provider is implemented by the native Rust source crate. Release packages
  contain no Python runtime, gallery-dl, OF-Scraper, bridge script, or provider-owned history store.
- Require TypeScript, the production build, the complete frontend and Rust test suites, command
  parity, strict Clippy, and source/artifact audits on the candidate. Record non-failing warnings
  separately rather than treating a passing exit code as a warning-free run.
- Check current production dependency advisories with `npm audit --omit=dev` and run the source
  license and repository-hygiene audit; packaged artifact licenses remain part of the platform gate.
- Keep the release candidate in reviewed commits and its worktree clean; generated build output
  remains untracked.
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
