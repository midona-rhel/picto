# Picto 0.6.0-alpha release readiness

Picto ships for these platforms only:

- macOS on Apple Silicon (`arm64`), distributed as DMG and ZIP artifacts;
- Windows on x86-64, distributed as an NSIS installer;
- Linux on x86-64, distributed as an AppImage.

Intel macOS is not a supported build or runtime target. Package configuration, CI artifact names,
native addons, and downloaded sidecars must not imply otherwise.

## Required gates

1. `npm run release:audit` passes before compilation.
2. TypeScript, focused UI tests, the production frontend build, Rust tests, and native-addon build
   pass on every supported runner.
3. `npm run release:audit -- --artifacts` passes after sidecars and native binaries are built.
4. The packaged smoke test opens the app with an isolated library, loads and settles the renderer,
   exercises both packaged sidecar self-tests, closes the native library, and removes its temporary
   data without native-module or sidecar errors. Import and thumbnail behavior remains covered by
   focused integration tests rather than being overstated as part of this smoke test.
5. Cloud Sync and Tutorials pass their separately owned persistence, accessibility, packaged-build,
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

- Cloud Sync and Tutorials are separately owned release gates and are intentionally not modified by
  release-finalization work.
- The accepted release icon is wired for macOS, Windows, and Linux. macOS packages consume the
  generated native `Picto.icon` layer bundle on an Apple Silicon macOS 26 runner; Apple `actool`
  compiles its Liquid Glass asset catalog and legacy ICNS representation during packaging.
- Code signing and notarization are not part of the `0.6.0-alpha` gate.
- The pull-request, manual, and tagged CI lanes build and smoke-test macOS Apple Silicon, Windows
  x64, and Linux x64. A normal push to `main` runs verification only. Windows and Linux still need
  one green clean-run result after the current branch is published.
- Gallery-dl and OF-Scraper source revisions and Python dependency graphs are frozen. The universal
  OnlyFans lock resolves for macOS ARM64, Windows x64, and Linux x64 on Python 3.12.
- TypeScript, the production build, and all 596 frontend tests pass. The suite still emits 242 React
  scheduling warnings, led by Settings, shared selects, Duplicate Review, and AI Tagging; remove the
  warning noise without suppressing React diagnostics before treating the gate as clean.
- `npm audit --omit=dev` reports no known production dependency vulnerabilities. The source license
  and repository-hygiene audit passes; packaged artifact licenses remain part of the platform gate.
- The large dirty integration surface must be separated into reviewed commits before publishing or
  tagging; generated build output must remain untracked.
- The public branch and tags have been rewritten to remove audited personal absolute paths,
  competitor references, copied audit material, and generated captures. Keep the local lineage on
  that scrubbed base before publishing further work so removed objects are not reintroduced.
