#!/usr/bin/env node

import { randomBytes } from 'node:crypto';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const SIGNING_IDENTITY = 'Picto Code Signing';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status ?? 'unknown'}`);
  }
  return result;
}

function signingCredentials(tempDirectory) {
  const configuredLink = process.env.MAC_CSC_LINK;
  const configuredPassword = process.env.MAC_CSC_KEY_PASSWORD;
  if (configuredLink || configuredPassword) {
    if (!configuredLink || !configuredPassword) {
      throw new Error('MAC_CSC_LINK and MAC_CSC_KEY_PASSWORD must be provided together.');
    }

    const configuredPath = path.resolve(configuredLink.replace(/^file:\/\//, ''));
    if (existsSync(configuredPath)) {
      return { certificatePath: configuredPath, password: configuredPassword };
    }

    const certificatePath = path.join(tempDirectory, 'picto-code-signing.p12');
    writeFileSync(certificatePath, Buffer.from(configuredLink, 'base64'), { mode: 0o600 });
    return { certificatePath, password: configuredPassword };
  }

  const signingDirectory = path.join(homedir(), 'Library', 'Application Support', 'Picto', 'signing');
  const certificatePath = path.join(signingDirectory, 'picto-code-signing.p12');
  const passwordPath = path.join(signingDirectory, '.p12-password');
  if (!existsSync(certificatePath) || !existsSync(passwordPath)) return null;

  return {
    certificatePath,
    password: readFileSync(passwordPath, 'utf8').trim(),
  };
}

function prepareMacSigning() {
  if (process.platform !== 'darwin') return null;

  const temporaryDirectory = mkdtempSync(path.join(tmpdir(), 'picto-signing-'));
  const credentials = signingCredentials(temporaryDirectory);
  if (!credentials) {
    rmSync(temporaryDirectory, { recursive: true, force: true });
    if (process.env.PICTO_REQUIRE_MAC_SIGNING === 'true') {
      throw new Error('A signed macOS build requires MAC_CSC_LINK and MAC_CSC_KEY_PASSWORD.');
    }
    return null;
  }

  const keychainPath = path.join(temporaryDirectory, 'picto-build.keychain-db');
  const keychainPassword = randomBytes(24).toString('hex');

  try {
    run('security', ['create-keychain', '-p', keychainPassword, keychainPath]);
    run('security', ['unlock-keychain', '-p', keychainPassword, keychainPath]);
    run('security', ['set-keychain-settings', '-lut', '21600', keychainPath]);
    run('security', [
      'import',
      credentials.certificatePath,
      '-k',
      keychainPath,
      '-P',
      credentials.password,
      '-T',
      '/usr/bin/codesign',
    ]);

    run('security', [
      'set-key-partition-list',
      '-S',
      'apple-tool:,apple:,codesign:',
      '-s',
      '-k',
      keychainPassword,
      keychainPath,
    ], { stdio: 'ignore' });
    run('security', ['find-identity', '-v', '-p', 'codesigning', keychainPath]);
  } catch (error) {
    spawnSync('security', ['delete-keychain', keychainPath], { stdio: 'ignore' });
    rmSync(temporaryDirectory, { recursive: true, force: true });
    throw error;
  }

  return {
    environment: {
      ...process.env,
      CSC_IDENTITY_AUTO_DISCOVERY: 'true',
      CSC_KEYCHAIN: keychainPath,
      CSC_NAME: SIGNING_IDENTITY,
    },
    cleanup() {
      spawnSync('security', ['delete-keychain', keychainPath], { stdio: 'ignore' });
      rmSync(temporaryDirectory, { recursive: true, force: true });
    },
  };
}

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

const args = [
  path.join(root, 'node_modules', 'electron-builder', 'out', 'cli', 'cli.js'),
  '--publish=never',
  ...process.argv.slice(2),
];
if (!supportsIconComposer()) {
  console.log('Xcode 26 Icon Composer is unavailable; packaging with the generated flat macOS icon.');
  args.push('--config.mac.icon=build/icon-flat.png');
}

const command = process.execPath;
const signing = prepareMacSigning();
const stop = (exitCode) => {
  signing?.cleanup();
  process.exit(exitCode);
};
process.once('SIGINT', () => stop(130));
process.once('SIGTERM', () => stop(143));
try {
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
