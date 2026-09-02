# Picto 0.6.9-alpha release readiness

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
   closes the native library, and removes its temporary data without native-module errors. Import,
   subscription, and thumbnail behavior remains covered by
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
- Release commits must contain no personal absolute paths, private-key markers, or reference-source
  references. Run the source audit before publishing history.

## Publication gates

- Cloud Sync and Tutorials retain separately owned packaged-build and restart-recovery gates.
- The accepted release icon is wired for macOS, Windows, and Linux. macOS packages consume the
  generated native `Picto.icon` layer bundle on an Apple Silicon macOS 26 runner; Apple `actool`
  compiles its Liquid Glass asset catalog and legacy ICNS representation during packaging.
- Public Apple Developer ID signing and notarization are not part of the `0.6.9-alpha` gate. The
  macOS alpha packages are signed with the project's private self-signed certificate so packaged
  binaries have one stable identity.
- The pull-request, manual, and tagged CI lanes build and smoke-test macOS Apple Silicon, Windows
  x64, and Linux x64. A normal push to `main` runs verification only. The Windows and Linux package
  results are publication gates and necessarily run after the release candidate is uploaded.
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
