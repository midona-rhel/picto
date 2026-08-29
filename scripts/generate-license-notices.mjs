#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(root, process.argv[2] ?? 'dist/licenses');
const licenseName = /^(license|licence|copying|notice|copyright)([._-].*)?$/i;

async function readableLicenseFiles(directory) {
  try {
    const entries = await readdir(directory, { withFileTypes: true });
    return entries
      .filter((entry) => entry.isFile() && licenseName.test(entry.name))
      .map((entry) => path.join(directory, entry.name))
      .sort();
  } catch {
    return [];
  }
}

function section(title, metadata, texts) {
  const rule = '-'.repeat(80);
  return [rule, title, ...metadata, rule, ...texts, ''].join('\n');
}

async function npmNotices() {
  const lock = JSON.parse(await readFile(path.join(root, 'package-lock.json'), 'utf8'));
  const packages = [];
  for (const [relativePath, locked] of Object.entries(lock.packages ?? {})) {
    if (!relativePath.startsWith('node_modules/')) continue;
    const directory = path.join(root, relativePath);
    let manifest = {};
    try {
      manifest = JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8'));
    } catch {
      // Optional packages for other operating systems are absent on this host. Their lock metadata
      // is still recorded below, and their own package is present on the platform that ships it.
    }
    const name = manifest.name ?? relativePath.slice(relativePath.lastIndexOf('node_modules/') + 13);
    const version = manifest.version ?? locked.version ?? 'unknown';
    const license = manifest.license ?? locked.license;
    if (!license) throw new Error(`NPM package ${name}@${version} has no declared license`);
    const files = await readableLicenseFiles(directory);
    const texts = files.length > 0
      ? await Promise.all(files.map(async (file) => `${path.basename(file)}\n\n${await readFile(file, 'utf8')}`))
      : ['The published package contains no separate license file; its package metadata declares the SPDX expression above.'];
    packages.push({ name, version, license, texts });
  }
  packages.sort((left, right) => left.name.localeCompare(right.name) || left.version.localeCompare(right.version));
  return [
    'Picto bundled JavaScript dependency notices',
    'Generated from package-lock.json and the installed package contents.',
    '',
    ...packages.map((entry) => section(
      `${entry.name}@${entry.version}`,
      [`License: ${entry.license}`],
      entry.texts,
    )),
  ].join('\n');
}

async function cargoNotices() {
  const raw = execFileSync('cargo', [
    'metadata', '--manifest-path', path.join(root, 'core/Cargo.toml'), '--locked', '--offline', '--format-version', '1',
  ], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  const metadata = JSON.parse(raw);
  const packages = [];
  for (const crate of metadata.packages) {
    if (!crate.license) throw new Error(`Rust crate ${crate.name}@${crate.version} has no declared license`);
    const directory = path.dirname(crate.manifest_path);
    const files = await readableLicenseFiles(directory);
    const texts = files.length > 0
      ? await Promise.all(files.map(async (file) => `${path.basename(file)}\n\n${await readFile(file, 'utf8')}`))
      : ['The crate contains no separate license file; its Cargo metadata declares the SPDX expression above.'];
    packages.push({ name: crate.name, version: crate.version, license: crate.license, texts });
  }
  packages.sort((left, right) => left.name.localeCompare(right.name) || left.version.localeCompare(right.version));
  return [
    'Picto bundled Rust dependency notices',
    'Generated from the locked Cargo dependency graph and crate sources.',
    '',
    ...packages.map((entry) => section(
      `${entry.name}@${entry.version}`,
      [`License: ${entry.license}`],
      entry.texts,
    )),
  ].join('\n');
}

await mkdir(output, { recursive: true });
await Promise.all([
  writeFile(path.join(output, 'NPM_THIRD_PARTY_NOTICES.txt'), await npmNotices()),
  writeFile(path.join(output, 'RUST_THIRD_PARTY_NOTICES.txt'), await cargoNotices()),
  copyFile(path.join(root, 'LICENSE'), path.join(output, 'PICTO_LICENSE.txt')),
  copyFile(path.join(root, 'THIRD_PARTY_LICENSES'), path.join(output, 'THIRD_PARTY_LICENSES.txt')),
  copyFile(path.join(root, 'src/shared/assets/fonts/LICENSE-Geist.txt'), path.join(output, 'GEIST_LICENSE.txt')),
  copyFile(path.join(root, 'src/shared/assets/fonts/LICENSE-FiraMono.txt'), path.join(output, 'FIRA_MONO_LICENSE.txt')),
  copyFile(path.join(root, 'src/shared/assets/fonts/LICENSE-Roboto.txt'), path.join(output, 'ROBOTO_LICENSE.txt')),
  copyFile(path.join(root, 'resources/tutorial/TUTORIAL_ASSETS.json'), path.join(output, 'TUTORIAL_ASSETS.json')),
]);

console.log(`Generated release notices in ${path.relative(root, output)}`);
