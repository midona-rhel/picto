#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const nativeRoot = path.join(root, 'native', 'picto-node');
const napi = path.join(nativeRoot, 'node_modules', '@napi-rs', 'cli', 'scripts', 'index.js');
const result = spawnSync(process.execPath, [napi, 'build', ...process.argv.slice(2)], {
  cwd: nativeRoot,
  stdio: 'inherit',
});

if (result.error) throw result.error;
if (result.status !== 0) {
  process.exitCode = result.status ?? 1;
} else {
  const stageScript = path.join(root, 'scripts', 'stage-native-runtime-libraries.mjs');
  const stage = spawnSync(process.execPath, [stageScript], { cwd: root, stdio: 'inherit' });
  if (stage.error) throw stage.error;
  process.exitCode = stage.status ?? 1;
}
