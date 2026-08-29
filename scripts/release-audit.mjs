#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const checkArtifacts = process.argv.includes('--artifacts');
const failures = [];
const assert = (condition, message) => { if (!condition) failures.push(message); };
const read = (relative) => readFileSync(path.join(root, relative), 'utf8');
const tracked = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
  .trim()
  .split('\n')
  .filter((file) => file && existsSync(path.join(root, file)));

for (const file of tracked) {
  assert(!file.startsWith('output/'), `tracked audit output must be removed: ${file}`);
  assert(!file.startsWith('.vite/'), `tracked Vite cache must be removed: ${file}`);
  assert(!file.startsWith('.tmp/'), `tracked temporary file must be removed: ${file}`);
  assert(file !== '.claude/settings.local.json', 'tracked local Claude settings must be removed');
  assert(file !== 'context-menu.js', 'copied reference context-menu source must not ship in Picto');
  assert(!file.endsWith('/.DS_Store') && file !== '.DS_Store', `tracked OS metadata must be removed: ${file}`);
}

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
  'build/Picto.icon/Assets/01-spine.png',
  'build/Picto.icon/Assets/02-under-pages.png',
  'build/Picto.icon/Assets/03-open-book.png',
]);
const macHomePrefix = `/${'Users'}/`;
const windowsHomePattern = new RegExp(`[A-Z]:\\\\${'Users'}\\\\`, 'i');
const privateKeyPattern = new RegExp(`-----BEGIN (RSA |EC |OPENSSH |DSA )?${'PRIVATE KEY'}-----`);
for (const file of tracked) {
  const extension = path.extname(file).toLowerCase();
  if (trackedMediaExtensions.has(extension)) {
    const approvedAsset = file.startsWith('src/shared/assets/')
      || file.startsWith('resources/tutorial/')
      || file.startsWith('tests/fixtures/')
      || file.startsWith('core/tests/fixtures/')
      || approvedReleaseIcons.has(file);
    assert(approvedAsset, `tracked media must be an approved product asset or test fixture: ${file}`);
  }
  if (!textExtensions.has(path.extname(file).toLowerCase())) continue;
  const contents = read(file);
  assert(!contents.includes(macHomePrefix), `tracked source contains a personal absolute path: ${file}`);
  assert(!windowsHomePattern.test(contents), `tracked source contains a personal Windows path: ${file}`);
  assert(!privateKeyPattern.test(contents), `tracked source contains a private key marker: ${file}`);
  assert(!contents.toLowerCase().includes(referenceProductName.toLowerCase()), `tracked source contains a reference-specific name: ${file}`);
}

const packageManifest = JSON.parse(read('package.json'));
assert(packageManifest.license === 'MIT', 'package.json must declare the project MIT license');
assert(packageManifest.build.mac.target.every((target) => target.arch?.length === 1 && target.arch[0] === 'arm64'), 'macOS packages must target Apple Silicon only');
assert(packageManifest.build.mac.icon === 'build/Picto.icon', 'macOS packages must use the native Icon Composer asset');
const libraryAssociation = packageManifest.build.fileAssociations?.find((association) => association.ext === 'library');
assert(libraryAssociation?.name === 'Picto Library', 'macOS packages must identify Picto library packages');
assert(libraryAssociation?.icon === 'library.icns', 'Picto library packages must use the custom library icon');
assert(libraryAssociation?.isPackage === true && libraryAssociation?.rank === 'Owner', 'Picto libraries must register as owned macOS document packages');
const windowsLibraryIcon = packageManifest.build.extraResources.find((resource) => resource.to === 'library-icons/library.ico');
assert(windowsLibraryIcon?.from === 'build/library.ico', 'Windows packages must include the custom library folder icon');
const macLibraryIcon = packageManifest.build.extraResources.find((resource) => resource.to === 'library-icons/library.icns');
assert(macLibraryIcon?.from === 'build/library.icns', 'macOS packages must include the custom library package icon');
assert(packageManifest.build.win.target.every((target) => target.arch?.length === 1 && target.arch[0] === 'x64'), 'Windows packages must target x64 only');
assert(packageManifest.build.linux.target.every((target) => target.arch?.length === 1 && target.arch[0] === 'x64'), 'Linux packages must target x64 only');
assert(packageManifest.build.files.includes('dist/licenses/**/*'), 'packaged files must include generated license notices');

const iconComposer = JSON.parse(read('build/Picto.icon/icon.json'));
const expectedMacIconLayers = ['01-spine.png', '02-under-pages.png', '03-open-book.png'];
assert(iconComposer.groups?.length === expectedMacIconLayers.length, 'native macOS icon must contain exactly three material groups');
for (const [index, imageName] of expectedMacIconLayers.entries()) {
  const group = iconComposer.groups?.[index];
  assert(group?.layers?.length === 1 && group.layers[0]?.['image-name'] === imageName, `native macOS icon layer order is invalid: ${imageName}`);
  assert(group?.layers?.[0]?.glass === true && group?.specular === true, `native macOS icon material is invalid: ${imageName}`);
}
for (const sidecar of ['gallery-dl', 'onlyfans']) {
  const entry = packageManifest.build.extraFiles.find((candidate) => candidate.to === `${sidecar}/`);
  assert(entry?.filter?.includes('THIRD_PARTY_LICENSES.txt'), `${sidecar} package must include its frozen Python notices`);
}

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

const runtimeRequirements = read('scripts/gallery-dl-runtime-requirements.txt');
const onlyFansRequirements = read('scripts/onlyfans-bridge-requirements.txt');
const floatingRequirement = /^\s*[^#\n]+(?:>=|~=|>|<)[^\n]*$/m;
assert(!floatingRequirement.test(runtimeRequirements), 'gallery-dl runtime requirements must be exact pins');
assert(!floatingRequirement.test(onlyFansRequirements), 'OnlyFans sidecar requirements must be exact pins');
for (const [name, requirements] of [
  ['gallery-dl', runtimeRequirements],
  ['OnlyFans', onlyFansRequirements],
]) {
  for (const url of requirements.matchAll(/https:\/\/[^\s]+\/archive\/([^/\s]+)\.zip/g)) {
    assert(/^[a-f0-9]{40}$/.test(url[1]), `${name} Git dependency must use a full commit SHA`);
  }
}

const releasePlan = read('docs/RELEASE_COMPLETION_PLAN.md');
assert(releasePlan.includes('Cloud Sync and Tutorials'), 'release plan must identify Cloud Sync and Tutorials as release gates');
assert(!releasePlan.includes('Cloud sync is deferred') && !releasePlan.includes('Cloud sync is absent'), 'release plan still claims Cloud Sync is deferred or absent');
assert(!existsSync(path.join(root, '.github/workflows/build.yml')), 'duplicate build workflow must be removed');
const readiness = read('docs/RELEASE_READINESS.md');
assert(readiness.includes('Apple Silicon (`arm64`)'), 'release readiness must identify Apple Silicon as the only macOS target');
assert(!readiness.includes('macOS on Intel'), 'release readiness must not advertise Intel macOS support');

const ffmpegDownload = read('scripts/download-ffmpeg.sh');
assert(ffmpegDownload.includes('verify_sha256'), 'FFmpeg downloads must verify pinned SHA-256 digests');

if (checkArtifacts) {
  for (const file of [
    'build/Picto.icon/icon.json',
    'build/Picto.icon/Assets/01-spine.png',
    'build/Picto.icon/Assets/02-under-pages.png',
    'build/Picto.icon/Assets/03-open-book.png',
    'build/library-folder.png',
    'build/library.icns',
    'build/library.ico',
    'dist/licenses/NPM_THIRD_PARTY_NOTICES.txt',
    'dist/licenses/RUST_THIRD_PARTY_NOTICES.txt',
    'vendor/gallery-dl/THIRD_PARTY_LICENSES.txt',
    'vendor/onlyfans/THIRD_PARTY_LICENSES.txt',
  ]) assert(existsSync(path.join(root, file)), `release artifact is missing: ${file}`);

  const executableSuffix = process.platform === 'win32' ? '.exe' : '';
  for (const file of [
    `vendor/ffmpeg/ffmpeg${executableSuffix}`,
    `vendor/ffmpeg/ffprobe${executableSuffix}`,
    `vendor/gallery-dl/picto-gallery-dl-bridge${executableSuffix}`,
    `vendor/onlyfans/picto-onlyfans-bridge${executableSuffix}`,
  ]) {
    const artifact = path.join(root, file);
    const exists = existsSync(artifact);
    assert(exists, `release runtime is missing: ${file}`);
    if (!exists) continue;
    const stat = statSync(artifact);
    assert(stat.isFile() && stat.size > 0, `release runtime is not a non-empty file: ${file}`);
    if (process.platform !== 'win32') {
      assert((stat.mode & 0o111) !== 0, `release runtime is not executable: ${file}`);
    }
  }

  if (process.platform === 'darwin') {
    const addon = path.join(root, 'native/picto-node/index.node');
    assert(existsSync(addon), 'release native addon is missing');
    if (existsSync(addon)) {
      const links = execFileSync('otool', ['-L', addon], { encoding: 'utf8' });
      assert(!links.includes('/opt/homebrew/') && !links.includes('/usr/local/'), 'native addon links to a build-host package-manager path');
    }
  }

  if (process.platform === 'linux') {
    const webGpuRuntime = path.join(root, 'native/picto-node/libwebgpu_dawn.so');
    assert(existsSync(webGpuRuntime), 'release WebGPU runtime is missing beside the native addon');
  }
}

if (failures.length > 0) {
  console.error(`Release audit failed (${failures.length}):`);
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}
console.log(`Release audit passed${checkArtifacts ? ' (source + artifacts)' : ' (source)'}.`);
