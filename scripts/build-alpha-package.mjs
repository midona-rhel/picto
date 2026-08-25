#!/usr/bin/env node

import { spawnSync } from 'node:child_process';

function supportsIconComposer() {
  if (process.platform !== 'darwin') return true;

  const result = spawnSync('xcrun', ['actool', '--version'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  if (result.status !== 0) return false;

  const match = result.stdout.match(/(?:actool|version)\s+(\d+)/i);
  return match !== null && Number(match[1]) >= 26;
}

const args = ['electron-builder', '--publish=never'];
if (!supportsIconComposer()) {
  console.log('Xcode 26 Icon Composer is unavailable; packaging with the generated flat macOS icon.');
  args.push('--config.mac.icon=build/icon-flat.png');
}

const command = process.platform === 'win32' ? 'npx.cmd' : 'npx';
const result = spawnSync(command, args, { stdio: 'inherit' });
process.exit(result.status ?? 1);
