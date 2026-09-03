#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dryRun = process.argv.includes('--dry-run');
const packageArgs = process.argv.slice(2).filter((argument) => argument !== '--dry-run');

function run(label, script, args = []) {
  const command = [process.execPath, path.join(root, script), ...args];
  console.log(`\n[alpha-package] ${label}`);
  if (dryRun) {
    console.log(command.map((part) => JSON.stringify(part)).join(' '));
    return;
  }
  const result = spawnSync(command[0], command.slice(1), { cwd: root, stdio: 'inherit' });
  if (result.error) throw new Error(`${label}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${label} exited with status ${result.status ?? 'unknown'}`);
}

run('Generate application icons', 'scripts/generate-app-icons.mjs');
run('Build native addon', 'scripts/build-native.mjs', ['--release']);
run('Prepare viewer assets', 'scripts/prepare-viewer-assets.mjs');
run('Compile TypeScript', 'node_modules/typescript/bin/tsc');
run('Build renderer', 'node_modules/vite/bin/vite.js', ['build']);
run('Generate license notices', 'scripts/generate-license-notices.mjs');
run('Audit release artifacts', 'scripts/release-audit.mjs', ['--artifacts']);
run('Build installers', 'scripts/build-alpha-package.mjs', packageArgs);
