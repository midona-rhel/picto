import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const nativeRoot = path.join(root, 'native', 'picto-node');
const napi = path.join(nativeRoot, 'node_modules', '@napi-rs', 'cli', 'scripts', 'index.js');

const result = spawnSync(process.execPath, [napi, 'build', ...process.argv.slice(2)], {
  cwd: nativeRoot,
  stdio: 'inherit',
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);

const staging = spawnSync(process.execPath, [path.join(root, 'scripts', 'stage-native-runtime-libraries.mjs')], {
  cwd: root,
  stdio: 'inherit',
});
if (staging.error) throw staging.error;
process.exitCode = staging.status ?? 1;
