#!/usr/bin/env node

import { spawn, spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const build = spawnSync(process.execPath, [path.join(root, 'scripts', 'build-native-dev.mjs')], {
  cwd: root,
  env: { ...process.env, CARGO_PROFILE_DEV_DEBUG: '0' },
  stdio: 'inherit',
});

if (build.error) throw build.error;
if (build.status !== 0) {
  process.exitCode = build.status ?? 1;
} else {
  const electronCli = path.join(root, 'node_modules', 'electron', 'cli.js');
  // Launch the project root so Electron reads Picto's package.json. Launching
  // the main module directly makes app.getVersion() report Electron's version.
  const entrypoint = process.argv[2] ?? root;
  const child = spawn(process.execPath, [electronCli, entrypoint], {
    cwd: root,
    env: process.env,
    stdio: 'inherit',
  });

  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.once(signal, () => child.kill(signal));
  }
  child.once('error', (error) => {
    console.error('[electron-dev] failed to launch Electron:', error);
    process.exitCode = 1;
  });
  child.once('exit', (code, signal) => {
    process.exitCode = code ?? (signal ? 1 : 0);
  });
}
