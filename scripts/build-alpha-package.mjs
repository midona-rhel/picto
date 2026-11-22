#!/usr/bin/env node

// Builds the Electron app's packages with electron-builder. On macOS the app
// is signed with the project's own certificate (`mac-signing.mjs`) and the
// signature is checked before the disk image and the archive are made
// (`verify-mac-package.mjs`).

import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { prepareMacSigning } from './mac-signing.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function supportsIconComposer(platform = process.platform) {
  if (platform !== 'darwin') return true;

  const result = spawnSync('xcodebuild', ['-version'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  if (result.status !== 0) return false;

  const match = result.stdout.match(/^Xcode\s+(\d+)/m);
  return match !== null && Number(match[1]) >= 26
    && spawnSync('xcrun', ['--find', 'actool'], { stdio: 'ignore' }).status === 0;
}

export function packageArguments({ platform, env, iconComposer, signing, extra = [] }) {
  const args = [
    path.join(root, 'node_modules', 'electron-builder', 'out', 'cli', 'cli.js'),
    '--publish=never',
    ...extra,
  ];
  if (platform === 'darwin' && !iconComposer) {
    if (env.PICTO_REQUIRE_MAC_SIGNING === 'true') {
      throw new Error('Release packaging requires Xcode 26+ and actool for the Icon Composer icon.');
    }
    console.log('Xcode 26 Icon Composer is unavailable; packaging with the generated flat macOS icon.');
    args.push('--config.mac.icon=build/icon-flat.png');
  }
  if (signing) args.push('--config.forceCodeSigning=true',
    `--config.mac.identity=${signing.environment.PICTO_MAC_SIGNING_IDENTITY_HASH}`,
    `--config.afterSign=${path.join(root, 'scripts/verify-mac-package.mjs')}`);
  return args;
}

export function main() {
  const command = process.execPath;
  const signing = prepareMacSigning();
  const stop = (exitCode) => {
    signing?.cleanup();
    process.exit(exitCode);
  };
  process.once('SIGINT', () => stop(130));
  process.once('SIGTERM', () => stop(143));
  try {
    const args = packageArguments({ platform: process.platform, env: process.env,
      iconComposer: supportsIconComposer(), signing, extra: process.argv.slice(2) });
    const result = spawnSync(command, args, {
      stdio: 'inherit',
      env: signing?.environment ?? process.env,
    });
    if (result.error) throw result.error;
    process.exitCode = result.status ?? 1;
  } finally {
    process.removeAllListeners('SIGINT');
    process.removeAllListeners('SIGTERM');
    signing?.cleanup();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) main();
