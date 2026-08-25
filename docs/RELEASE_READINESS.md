# Release readiness

Picto ships for these platforms only:

- macOS on Apple Silicon (`arm64`), distributed as signed and notarized DMG and ZIP artifacts;
- Windows on x86-64, distributed as a signed NSIS installer;
- Linux on x86-64, distributed as an AppImage.

Intel macOS is not a supported build or runtime target. Package configuration, CI artifact names,
native addons, and downloaded sidecars must not imply otherwise.

## Required gates

1. `npm run release:audit` passes before compilation.
2. TypeScript, focused UI tests, the production frontend build, Rust tests, and native-addon build
   pass on every supported runner.
3. `npm run release:audit -- --artifacts` passes after sidecars and native binaries are built.
4. Tagged macOS and Windows builds fail if signing credentials are absent. macOS artifacts are
   notarized before publication.
5. The packaged smoke test opens the app with an isolated library, loads and settles the renderer,
   exercises both packaged sidecar self-tests, closes the native library, and removes its temporary
   data without native-module or sidecar errors. Import and thumbnail behavior remains covered by
   focused integration tests rather than being overstated as part of this smoke test.
6. Cloud Sync and Tutorials pass their separately owned persistence, accessibility, packaged-build,
   and restart-recovery gates.

## Repository hygiene

- Do not commit generated binaries, databases, credentials, local settings, screenshots, audit
  captures, build caches, or extracted third-party application source.
- Product and test assets live under an explicit asset or fixture directory and carry provenance
  and license information when Picto did not create them.
- Third-party runtimes are fetched from pinned revisions or release URLs, verified where publisher
  digests exist, and accompanied by version-exact notices.
- Release commits must contain no personal absolute paths, private-key markers, or competitor-source
  references. Run the source audit before publishing history.

## Outstanding release blockers

- Cloud Sync and Tutorials are not complete in this workstream.
- The accepted release icon is wired for macOS, Windows, and Linux. The macOS package currently
  uses the generated flattened ICNS fallback; an Apple Icon Composer export remains the final
  material pass when that Apple tool is available.
- Signing and notarization can only be proven in the tagged CI environment with release secrets.
- The pull-request, manual, and tagged CI lanes build and smoke-test macOS Apple Silicon, Windows
  x64, and Linux x64. A normal push to `main` runs verification only. Windows and Linux still need
  one green clean-run result after the current branch is published.
- Gallery-dl and OF-Scraper source revisions and Python dependency graphs are frozen. The universal
  OnlyFans lock resolves for macOS ARM64, Windows x64, and Linux x64 on Python 3.12.
- The frontend suite passes but emits existing React `act(...)` warnings from asynchronous Settings,
  AI Tagger, and Duplicate Review tests; remove this warning noise before treating logs as clean.
- The repository is already public. Before the local branch is published, rewrite reachable history
  to remove personal absolute paths, competitor references, copied audit material, and generated
  captures. A later deletion commit is insufficient because the old blobs remain reachable.
