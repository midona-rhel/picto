import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const python = process.platform === 'win32' ? 'python' : 'python3';

function run(label, command, args, cwd = root) {
  console.log(`\n[alpha-package] ${label}`);
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.error) throw new Error(`${label}: ${result.error.message}`);
  if (result.status !== 0) {
    throw new Error(`${label} exited with status ${result.status ?? 'unknown'}`);
  }
}

const node = (label, script, args = []) =>
  run(label, process.execPath, [path.join(root, script), ...args]);

node('Generate application icons', 'scripts/generate-app-icons.mjs');
node('Build native addon', 'scripts/build-native.mjs', ['--release']);
node('Compile TypeScript', 'node_modules/typescript/bin/tsc');
node('Build renderer', 'node_modules/vite/bin/vite.js', ['build']);
node('Generate license notices', 'scripts/generate-license-notices.mjs');
run('Build gallery downloader', python, [path.join(root, 'scripts/build-gallery-dl-bridge.py')]);
run('Build subscription sidecar', python, [path.join(root, 'scripts/build-onlyfans-bridge.py')]);
node('Audit release artifacts', 'scripts/release-audit.mjs', ['--artifacts']);
node('Build installers', 'scripts/build-alpha-package.mjs');
