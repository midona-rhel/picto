#!/usr/bin/env node

// Checks that the repository, or a built package, is fit for a release.
//
//   node scripts/release-audit.mjs              checks the tracked sources
//   node scripts/release-audit.mjs --artifacts  checks the package that
//                                               `package.sh` made on this system
//
// Every failed check is printed, and the script then ends with status 1.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const checkArtifacts = process.argv.includes('--artifacts');
const failures = [];
const assert = (condition, message) => { if (!condition) failures.push(message); };
const read = (relative) => readFileSync(path.join(root, relative), 'utf8');
const packageManifest = JSON.parse(read('package.json'));

// Checks the package in `experiments/gpui/target/package`. A release
// publishes these files under these names, and an installed Picto finds its
// update through the feed. The folder must therefore hold exactly these
// files, and the feed must give this version and each package's real size
// and SHA-512.
function auditPackage() {
  const version = packageManifest.version;
  const expected = {
    darwin: { feed: 'latest-mac.yml', packages: [`Picto-${version}-arm64-mac.zip`, `Picto-${version}-arm64.dmg`] },
    win32: { feed: 'latest.yml', packages: [`Picto-Setup-${version}.exe`] },
    linux: { feed: 'latest-linux.yml', packages: [`Picto-${version}.AppImage`] },
  }[process.platform];
  if (!expected) {
    failures.push(`there is no package for the system ${process.platform}`);
    return;
  }
  const folder = path.join(root, 'experiments/gpui/target/package');
  if (!existsSync(folder)) {
    failures.push('experiments/gpui/target/package does not exist; run package.sh first');
    return;
  }
  const found = readdirSync(folder, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort();
  const wanted = [...expected.packages, expected.feed].sort();
  assert(JSON.stringify(found) === JSON.stringify(wanted),
    `the package folder must hold exactly ${wanted.join(', ')}; it holds ${found.join(', ') || 'no files'}`);
  if (!found.includes(expected.feed)) return;

  // The feed is the text that `write_feed` in `package.sh` writes: a line
  // `version:`, then for each package the lines `- url:`, `sha512:` and
  // `size:`, then `path:` with the name of the first package.
  const feed = readFileSync(path.join(folder, expected.feed), 'utf8');
  const value = (pattern) => feed.match(pattern)?.[1]?.trim();
  assert(value(/^version: (.+)$/m) === version, `${expected.feed} must give the version ${version}`);
  assert(value(/^path: (.+)$/m) === expected.packages[0], `${expected.feed} must name ${expected.packages[0]} as its path`);
  const entries = [...feed.matchAll(/^  - url: (.+)\r?\n    sha512: (.+)\r?\n    size: (\d+)\r?$/gm)]
    .map(([, url, sha512, size]) => ({ url: url.trim(), sha512: sha512.trim(), size: Number(size) }));
  assert(JSON.stringify(entries.map((entry) => entry.url)) === JSON.stringify(expected.packages),
    `${expected.feed} must list ${expected.packages.join(', ')}`);
  for (const entry of entries) {
    const file = path.join(folder, entry.url);
    if (!existsSync(file)) continue;
    assert(statSync(file).size > 0 && statSync(file).size === entry.size, `${expected.feed} gives the wrong size for ${entry.url}`);
    assert(createHash('sha512').update(readFileSync(file)).digest('base64') === entry.sha512,
      `${expected.feed} gives the wrong SHA-512 for ${entry.url}`);
  }
}

// Checks the tracked sources: no file that must not be published, no
// personal path, private key or name of the reference product in a text
// file, a license for every dependency, and the facts that a release
// depends on.
function auditSource() {
  const tracked = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
    .trim()
    .split('\n')
    .filter((file) => file && existsSync(path.join(root, file)));

  // The subscription sources are implemented in Rust. The Python programs
  // that ran them before must not come back.
  const removedSubscriptionRuntimePaths = [
    'core/src/gallery_dl_source.rs',
    'core/src/onlyfans_source.rs',
    'core/src/sidecar_process.rs',
    'core/src/subscriptions/gallery_dl_runner.rs',
    'scripts/gallery_dl_bridge.py',
    'scripts/onlyfans_bridge.py',
  ];
  for (const file of removedSubscriptionRuntimePaths) {
    assert(!existsSync(path.join(root, file)), `removed subscription runtime returned: ${file}`);
  }

  // Output of tools, local settings and files of the operating system are
  // not tracked.
  for (const file of tracked) {
    assert(!file.startsWith('output/'), `tracked audit output must be removed: ${file}`);
    assert(!file.startsWith('.tmp/'), `tracked temporary file must be removed: ${file}`);
    assert(file !== '.claude/settings.local.json', 'tracked local Claude settings must be removed');
    assert(file !== 'context-menu.js', 'copied reference context-menu source must not ship in Picto');
    assert(!file.endsWith('/.DS_Store') && file !== '.DS_Store', `tracked OS metadata must be removed: ${file}`);
  }

  // A tracked picture, sound or video must be one of the app's icons or lie
  // in a folder of product assets or test fixtures. A tracked text file must
  // hold no home-folder path, no private key and not the name of the
  // reference product. The words that are searched for are put together
  // here from parts, so that this file does not hold them itself.
  const textExtensions = new Set(['.c', '.css', '.html', '.js', '.json', '.md', '.mjs', '.py', '.rs', '.sh', '.toml', '.ts', '.tsx', '.txt', '.yml', '.yaml']);
  const referenceProductName = String.fromCharCode(69, 97, 103, 108, 101);
  const trackedMediaExtensions = new Set([
    '.avif', '.bmp', '.gif', '.heic', '.icns', '.ico', '.jpeg', '.jpg', '.jxl', '.png', '.tif', '.tiff', '.webp',
    '.aac', '.flac', '.m4a', '.mp3', '.ogg', '.wav', '.m4v', '.mkv', '.mov', '.mp4', '.webm',
  ]);
  const approvedReleaseIcons = new Set([
    'build/icon-flat.png',
    'build/icon.ico',
    'build/library-folder.png',
    'build/library.icns',
    'build/library.ico',
    'build/picto-pack.png',
    'build/picto-pack.icns',
    'build/picto-pack.ico',
  ]);
  const macHomePrefix = `/${'Users'}/`;
  const windowsHomePattern = new RegExp(`[A-Z]:\\\\${'Users'}\\\\`, 'i');
  const privateKeyPattern = new RegExp(`-----BEGIN (RSA |EC |OPENSSH |DSA )?${'PRIVATE KEY'}-----`);
  for (const file of tracked) {
    const extension = path.extname(file).toLowerCase();
    if (trackedMediaExtensions.has(extension)) {
      const approvedAsset = file.startsWith('resources/tutorial/')
        || file.startsWith('core/tests/fixtures/')
        || file.startsWith('experiments/gpui/assets/fixtures/')
        || approvedReleaseIcons.has(file);
      assert(approvedAsset, `tracked media must be an approved product asset or test fixture: ${file}`);
    }
    if (!textExtensions.has(extension)) continue;
    const contents = read(file);
    assert(!contents.includes(macHomePrefix), `tracked source contains a personal absolute path: ${file}`);
    assert(!windowsHomePattern.test(contents), `tracked source contains a personal Windows path: ${file}`);
    assert(!privateKeyPattern.test(contents), `tracked source contains a private key marker: ${file}`);
    assert(!contents.toLowerCase().includes(referenceProductName.toLowerCase()), `tracked source contains a reference-specific name: ${file}`);
  }

  // The app shows the release notes of its own version, and a release uses
  // them as its text. The build reads them from this file.
  assert(packageManifest.license === 'MIT', 'package.json must declare the project MIT license');
  const releaseNotesPath = `docs/${packageManifest.version}-release-notes.md`;
  assert(existsSync(path.join(root, releaseNotesPath)) && read(releaseNotesPath).trim().length > 0, 'current version must have bundled release notes');

  // The macOS icon is one Icon Composer group that holds the book as one
  // glass layer.
  const iconComposer = JSON.parse(read('build/Picto.icon/icon.json'));
  const expectedMacIconLayers = ['01-book.png'];
  assert(iconComposer.groups?.length === expectedMacIconLayers.length, 'native macOS icon must contain the book as one material group');
  for (const [index, imageName] of expectedMacIconLayers.entries()) {
    const group = iconComposer.groups?.[index];
    assert(group?.layers?.length === 1 && group.layers[0]?.['image-name'] === imageName, `native macOS icon layer order is invalid: ${imageName}`);
    assert(group?.layers?.[0]?.glass === true && group?.specular === true, `native macOS icon material is invalid: ${imageName}`);
  }

  // Every npm package of the build scripts and every crate of the root
  // Cargo workspace declares a license.
  const packageLock = JSON.parse(read('package-lock.json'));
  for (const [location, dependency] of Object.entries(packageLock.packages ?? {})) {
    if (!location.startsWith('node_modules/')) continue;
    assert(Boolean(dependency.license), `NPM lock entry has no declared license: ${location}@${dependency.version ?? 'unknown'}`);
  }
  const cargoMetadata = JSON.parse(execFileSync('cargo', [
    'metadata', '--manifest-path', path.join(root, 'core/Cargo.toml'), '--locked', '--offline', '--format-version', '1',
  ], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }));
  for (const crate of cargoMetadata.packages) {
    assert(Boolean(crate.license), `Cargo package has no declared license: ${crate.name}@${crate.version}`);
  }

  // The AI models that the app offers for download are the ones the model
  // catalog marks as released, each pinned to one revision and with its
  // license named. A model whose terms are not stated is downloaded from
  // its publisher only and is not hosted by Picto as a Core ML file.
  const modelCatalog = JSON.parse(read('scripts/ai/model-catalog.json'));
  const modelRuntime = read('core/src/ai_tagger/models.rs');
  const coremlArtifacts = JSON.parse(read('scripts/ai/coreml-artifacts.json')).assets ?? {};
  for (const model of modelCatalog.models ?? []) {
    const label = `${model.slug ?? 'unnamed model'}`;
    const released = model.release_distribution === true;
    const registered = modelRuntime.includes(`slug: "${model.slug}".into()`);
    assert(/^[a-f0-9]{40}$/.test(model.revision ?? ''), `AI model revision must be a full commit SHA: ${label}`);
    assert(/^https:\/\//.test(model.license_url ?? ''), `AI model must identify its authoritative license page: ${label}`);
    assert(typeof model.release_distribution === 'boolean', `AI model must declare release_distribution: ${label}`);
    assert(registered === released, `AI model release registry disagrees with model catalog: ${label}`);
    if (!released) {
      assert(!(model.slug in coremlArtifacts), `excluded AI model has a Picto-hosted Core ML artifact: ${label}`);
      continue;
    }
    assert(
      modelRuntime.includes(`/resolve/${model.revision}/`),
      `released AI model runtime URL is not pinned to its catalog revision: ${label}`,
    );
    if (model.license === 'NOASSERTION') {
      assert(model.delivery === 'upstream-download', `AI model without explicit terms must be upstream-download only: ${label}`);
      assert(!(model.slug in coremlArtifacts), `AI model without explicit terms cannot have a Picto-hosted Core ML artifact: ${label}`);
    } else {
      assert(Boolean(model.license), `released AI model has no declared license: ${label}`);
    }
  }
  for (const slug of Object.keys(coremlArtifacts)) {
    const model = modelCatalog.models?.find((candidate) => candidate.slug === slug);
    assert(model?.release_distribution === true, `Core ML artifact is not backed by a released model: ${slug}`);
    assert(model?.license && model.license !== 'NOASSERTION', `Core ML artifact lacks explicit redistribution terms: ${slug}`);
  }

  // One workflow builds the packages (`package.yml`). Picto is released for
  // Apple Silicon only on macOS, and the `ffmpeg` that a package holds is
  // downloaded with a checked SHA-256.
  assert(!existsSync(path.join(root, '.github/workflows/build.yml')), 'duplicate build workflow must be removed');
  const readiness = read('docs/RELEASE_READINESS.md');
  assert(readiness.includes('Apple Silicon (`arm64`)'), 'release readiness must identify Apple Silicon as the only macOS target');
  assert(!readiness.includes('macOS on Intel'), 'release readiness must not advertise Intel macOS support');
  const ffmpegDownload = read('scripts/download-ffmpeg.sh');
  assert(ffmpegDownload.includes('verify_sha256'), 'FFmpeg downloads must verify pinned SHA-256 digests');
}

if (checkArtifacts) auditPackage();
else auditSource();

if (failures.length > 0) {
  console.error(`Release audit failed (${failures.length}):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}
console.log(`Release audit passed${checkArtifacts ? ' (package)' : ' (source)'}.`);
