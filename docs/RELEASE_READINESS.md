# Picto release readiness

Picto is released for these systems only:

- macOS on Apple Silicon (`arm64`), as a disk image and a ZIP archive;
- Windows on x86-64, as an NSIS installer;
- Linux on x86-64, as an AppImage.

Picto is not built for and does not run on a Mac with an Intel processor. No package setting, no
artifact name and no downloaded tool may suggest that it does.

## Required gates

A release passes all of these on its final commit. The Alpha Gate (`.github/workflows/alpha-gate.yml`)
runs them for every push to a release branch.

1. The source audit passes: `node scripts/release-audit.mjs`.
2. The `verify` workflow passes: the tests of the scripts, Clippy and the tests of the crates
   `core`, `picto-library` and `picto-sources`, and the build and the tests of the app on Linux
   and Windows.
3. The `Package` workflow builds the package for each of the three systems with `app/package.sh`,
   and each package passes that workflow's checks and the package audit
   (`node scripts/release-audit.mjs --artifacts`). The section "Packages" says what the checks
   prove.

What the gates do not prove:

- CI does not run the app's tests on macOS. The macOS job of the `Package` workflow compiles the
  release program there and starts it with `--version`. The tests are run on a Mac by hand
  (`cargo test --locked` in `app/`).
- No check opens a window or a library in a packaged program. The checks of the `Package` workflow
  start the packaged program with `--version` only, which ends before a window opens. The windows
  of the app are drawn and photographed by `verify:app`, which runs a debug build.
- Cloud Sync (experimental) is checked by the step `Cloud` of `verify:app`, against a plain folder
  that stands in for the desktop folder of Dropbox. It is not checked against a real provider.

## Repository hygiene

- Do not commit generated programs, databases, credentials, local settings, screenshots, captures
  of an audit, build caches, or the extracted source of another application.
- Product assets and test assets are in a folder that the source audit names as a folder of assets
  or fixtures. An asset that Picto did not create has its origin and its license written beside
  it.
- A program or a library that is downloaded for a build is taken from a fixed revision or a fixed
  release address, and is compared with the digest that its publisher gives where there is one.
- A commit of a release holds no home-folder path, no private key and not the name of the
  reference product. The source audit checks that. Run it before history is published.

## Packages

`app/package.sh` builds the release program and makes the package for the system it runs on.
[`app/README.md`](../app/README.md), section 3, says what each package holds and what the script
needs. The `Package` workflow (`.github/workflows/package.yml`) is the only workflow that builds
packages. It runs the script on macOS 26, Ubuntu 24.04 and Windows and uploads the results as the
artifacts `picto-macos-arm64`, `picto-linux-x64` and `picto-windows-x64`.

A release publishes these files:

| System | Files |
|---|---|
| macOS | `Picto-<version>-arm64.dmg`, `Picto-<version>-arm64-mac.zip`, `latest-mac.yml` |
| Windows | `Picto-Setup-<version>.exe`, `latest.yml` |
| Linux | `Picto-<version>.AppImage`, `latest-linux.yml` |

The `.yml` files are the update feeds. An installed Picto reads the feed of its system from the
newest release and finds its update there. Picto 0.6.19 and earlier, which were built with
Electron, read the same feeds under the same names, and the app has the same name and the same id
(`com.picto.desktop`) as those versions. A package therefore installs over such a version as its
next version.

What the `Package` workflow checks on each system:

- macOS: the archive is unpacked into a new folder and the disk image is mounted, and the program
  in each is run with `--version`. The unpacked app must pass `codesign --verify --deep --strict`
  and hold the compiled icon catalog.
- Linux: the AppImage is unpacked and the program is run with `--version`. Then the AppImage
  itself is run with `--version` in a container that holds only Ubuntu 24.04 and the libraries
  that the user guide names. The app's own reader checks the update feed against the AppImage.
- Windows: the installer is run silently as a first installation. The folder, the registry values,
  the shortcuts and the file association are checked, the installed program is run with
  `--version`, the installer is run again as an upgrade, and the uninstaller must remove
  everything. Then the installer of Picto 0.6.19-alpha is run, this installer upgrades that
  installation, and the folder must then hold this app and none of the old version's files.
- On every system, the package audit checks that the package folder holds exactly the files of
  the table above, and that the update feed gives the version and each file's size and SHA-512.

Signing:

- macOS: the app is signed with the project's own self-signed certificate "Picto Code Signing",
  from the secrets `MAC_CSC_LINK` and `MAC_CSC_KEY_PASSWORD` (`scripts/sign-mac-app.mjs`,
  `scripts/mac-signing.mjs`). An ad-hoc signature is never accepted in its place. In a release
  the macOS job fails without the certificate, and it fails without Xcode 26 or later, whose
  `actool` compiles the icon `build/Picto.icon`. Before the archive and the disk image are made,
  the script checks the app's signature, the certificate it was made with, and the compiled icon
  catalog. Outside a release, a run without the secrets leaves the app unsigned and prints that
  it is not signed; such a package must not be published.
- The self-signed certificate is trusted only on a GitHub-hosted macOS runner, which is thrown
  away after the job. The build deletes the private key and its temporary keychain and puts the
  keychain search list back as it was. A local or self-hosted machine must already trust the
  certificate.
- The app is not notarized, because the project has no Apple Developer ID.
- Windows and Linux: the installer and the AppImage are not signed. The project has no Windows
  code-signing certificate.

What a user sees when starting a package for the first time is written for users in
[Getting Started](user-guide/getting-started.md). In short:

- macOS: a downloaded Picto is not notarized, so macOS refuses to open it the first time and
  offers no button to open it. The user allows it once under System Settings › Privacy &
  Security with **Open Anyway**.
- Windows: Microsoft Defender SmartScreen shows "Windows protected your PC" for the installer,
  because it is not signed. The user chooses **More info** and then **Run anyway**. Where Smart
  App Control is on, Windows blocks the installer and offers no way to run it.
- Linux: the AppImage is a file the user makes executable. Nothing warns. It needs GTK 3 and
  WebKitGTK 4.1 on the system. The desktop images of Ubuntu 24.04 and Xubuntu 24.04 hold both;
  the one of Kubuntu 24.04 holds no WebKitGTK 4.1. When a library is missing, the AppImage's
  start script (`app/packaging/linux/AppRun`) shows a message that names the library and the
  package to install, and Picto does not start.

What a user who upgrades from Picto 0.6.19 or earlier keeps and loses:

- Kept: every library, and everything that is stored in a library, which includes the
  application settings. The application data folder (`picto` in the per-user application data
  folder), with the downloaded AI models. The site logins, which are kept in the system keychain
  under the same names.
- The list of libraries: the app copies the older version's list of libraries once, on its
  first start, and opens the library that was open last.
- Not carried over: what the older version kept per machine in its window's storage, which is
  the window's place and size, the widths of the sidebar and the inspector, the saved filters,
  the recent tags and folders, the keyboard preset and the changed shortcuts, the language and
  the interface zoom. The app keeps these in its own file, `state.json` in the folder
  `Picto GPUI` of the per-user configuration folder, and starts with the defaults.
- On Linux, the AppImage of Picto 0.6.19 and earlier needed no libraries of the system. This
  AppImage needs the ones named above.

Open before the first release that is built from these packages:

- The packages hold `LICENSE`, `THIRD_PARTY_LICENSES` and the license texts that file names, in
  a folder `licenses`. They do not hold the license texts of the other crates the app is built
  from.
- No `*.blockmap` files are made. An updater that does not find one downloads the whole package.
- No package has been installed and used with a library on any system, and no update has been
  installed from a published feed. [`app/README.md`](../app/README.md), section 7, lists what
  was not verified.

## Publication rules

- A pull request into a `release/**` branch runs the `verify` workflow. A push to a release
  branch runs the `verify` workflow and the `Package` workflow for the three systems side by
  side. A tag publishes the packages that the push of the same commit to its release branch
  built. A tag builds nothing. The rules for branches and pull requests are in
  [CONTRIBUTING](../CONTRIBUTING.md).
- Every subscription source is implemented by the Rust crate `picto-sources`. A package holds no
  Python runtime, no gallery-dl, no OF-Scraper, no bridge script and no history store of a
  provider.
- A check that passes with warnings is recorded with its warnings. An exit code of 0 is not
  reported as a run without warnings.
- The release candidate consists of reviewed commits, and its worktree is clean. Build output is
  not tracked.
- The public branches and tags were rewritten once to remove personal absolute paths, names of
  the reference product, copied audit material and generated captures. Base further work on that
  rewritten history, so that the removed objects are not published again.

## Release procedure

### Prepare locally, then review

Preparing a release and publishing it are separate actions. Preparing means making the local
commits and the release notes and reporting which gates are still open. It ends there. A release
branch or a tag is pushed only after that work was reviewed and the push was approved, because
pushing the branch starts the packaging and pushing the tag starts the publication.

Before choosing the next version, look at `git status`, `git log` and the remote's branches and
tags (`git ls-remote --heads --tags origin`). Local tags alone do not show what the remote holds.
Leave unrelated work alone, and stage only the files of the release: no generated libraries, no
benchmark output and no files of a local investigation.

Implementation reaches `dev` through reviewed pull requests ([CONTRIBUTING](../CONTRIBUTING.md)).
`main` holds released code only. A release is prepared on `release/<version>`, which is cut from
the accepted commit of `dev`, or from `main` for a hotfix. Published history is never rewritten
to prepare a release.

The version is written in these places. Change all of them to the new version, and change no
other dependency:

- `package.json`, and the two entries of the root package in `package-lock.json`. The app's
  build reads the version from `package.json`.
- `core/Cargo.toml` and `picto-sources/Cargo.toml`.
- The entries of `picto_core` and `picto_sources` in `Cargo.lock` and in `app/Cargo.lock`.

`picto-library` and the app's crate (`app/Cargo.toml`) have versions of their own, which do not
follow the release.

Create `docs/<version>-release-notes.md` with the heading `# Picto <version>` and short lines
that start with `* `, as the notes of the earlier versions have. The notes describe what a user
sees: no implementation details, no issue numbers, no test counts. Say plainly when an upgrade
changes what a user has or how the app starts. Keep the notes of older versions. The app's build
includes this file, the app shows it as What's New, and the release on GitHub uses it as its
text.

Run the source audit and `git diff --check` on the complete commit, including new files. Record
which checks passed on this commit and which were not run. A result from an earlier commit is
not reported as a result for this one. A local check does not replace a check that CI is set up
to run.

For a change to how a library is opened or switched, also check these four cases in a packaged
build on a generated library: opening, switching, quitting while a library loads, and opening
again. Record which process held the library, how long each step was given, and the result. The
checks of the `Package` workflow do not cover these cases.

Hand over for review: the ids of the local commits, the proposed version, the release notes, the
checks that are still open, and whether anything was pushed.

### Run the release gate and publish after approval

1. Cut `release/<version>` from the accepted commit of `dev` and push it. Commit the changed
   versions and the release notes on that branch. Issue references belong in commits, not in the
   release notes. Every push to the branch runs the Alpha Gate: the `verify` workflow and the
   `Package` workflow for macOS, Windows and Linux. Every job must pass on the final head.
2. When the gate has passed on the final head, open a pull request from `release/<version>` into
   `main` and merge it with a **merge commit**. Squash merges are switched off for `main`, so
   that the commit that passed the gate stays in `main`'s ancestry. The required check
   `branch-policy` looks for that commit's successful gate and its three package artifacts
   before the merge. If the pull request was opened before the gate finished, run its CI again
   after the gate has passed.
3. Tag the head of the release branch, not the merge commit, as `v<version>` and push the tag.
   The publication job checks that the tag is the head of its release branch, that the commit is
   in `main`, that the gate of that commit passed, and that its three package artifacts exist
   (`scripts/ci/find-release-gate.mjs`). It downloads those artifacts by their ids, compares
   their digests, and creates the release with the files and the release notes.
4. Open a pull request from `main` into `dev` and merge it with a merge commit, so that the
   fixes made on the release branch and the new version reach `dev`. Release branches are kept.

A hotfix follows the same steps, with `release/<version>` cut from `main`.

No test, lint, audit, signing requirement or package check is skipped to make the gate shorter.
The packages are built while the verification runs, so a candidate that fails verification has
used runner time for packages that are thrown away. The packages are built once, before the tag.

If a gate fails, fix it on the release branch through a pull request, and carry the fix to `dev`
in step 4. Then wait for the new gate. Run a failed job again only when the failure came from
the runner and not from the code. If the publication fails, run only the publication job of the
tag again. Do not move a published tag and do not build other files under it. If the artifacts
of the gate have expired, run the gate of the release branch again at the same commit before
publishing.

Look at the exact run of the Alpha Gate on GitHub. A run started by hand helps to find a fault,
but it is not the run that `scripts/ci/find-release-gate.mjs` requires, which is the run for the
push of the release branch. The tag must match the version in `package.json`, the release branch
must point to the same commit, and the commit must be in `main`. The three package artifacts
must exist and must not have expired. Do not create a release by hand from files that were built
locally, and do not use notes that GitHub generates in place of the release notes file.

After the publication, check the release's text, the files of each system and the three update
feeds. Keep the link to the successful run with the hand-over of the release. Do not state how
fast the app is on a system on which that was not measured.
