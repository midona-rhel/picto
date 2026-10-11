#!/usr/bin/env node

import { randomBytes } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const SIGNING_IDENTITY = 'Picto Code Signing';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    timeout: 30_000,
    killSignal: 'SIGKILL',
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args[0]} failed: ${result.error?.code ?? `exit ${result.status ?? 'unknown'}`}`);
  }
  return result;
}

function signingCredentials(tempDirectory, env) {
  const configuredLink = env.MAC_CSC_LINK;
  const configuredPassword = env.MAC_CSC_KEY_PASSWORD;
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

export function signingIdentity(output) {
  return output.match(/^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"Picto Code Signing"\s*$/m)?.[1] ?? null;
}

export function prepareMacSigning(env = process.env, platform = process.platform) {
  if (platform !== 'darwin') return null;

  const temporaryDirectory = mkdtempSync(path.join(tmpdir(), 'picto-signing-'));
  let credentials;
  try {
    credentials = signingCredentials(temporaryDirectory, env);
  } catch (error) {
    rmSync(temporaryDirectory, { recursive: true, force: true });
    throw error;
  }
  if (!credentials) {
    rmSync(temporaryDirectory, { recursive: true, force: true });
    if (env.PICTO_REQUIRE_MAC_SIGNING === 'true') {
      throw new Error('A signed macOS build requires MAC_CSC_LINK and MAC_CSC_KEY_PASSWORD.');
    }
    return null;
  }

  const keychainPath = path.join(temporaryDirectory, 'picto-build.keychain-db');
  const keychainPassword = randomBytes(24).toString('hex');
  const publicCertificate = path.join(temporaryDirectory, 'signing.cer');
  let temporaryTrust = false;
  let originalKeychains = null;
  const cleanup = () => {
    try {
      if (temporaryTrust) {
        run('sudo', ['security', 'remove-trusted-cert', '-d', publicCertificate], { stdio: 'ignore' });
        temporaryTrust = false;
      }
    } finally {
      try {
        if (originalKeychains !== null) {
          run('security', ['list-keychains', '-d', 'user', '-s', ...originalKeychains]);
          originalKeychains = null;
        }
      } finally {
        try {
          run('security', ['delete-keychain', keychainPath], { stdio: 'ignore' });
        } finally {
          rmSync(temporaryDirectory, { recursive: true, force: true });
        }
      }
    }
  };
  let identity;
  const findIdentity = () => signingIdentity(run('security', [
    'find-identity', '-v', '-p', 'codesigning', keychainPath,
  ], { encoding: 'utf8', stdio: 'pipe' }).stdout);

  try {
    const keychains = run('security', ['list-keychains', '-d', 'user'], {
      encoding: 'utf8', stdio: 'pipe',
    }).stdout;
    originalKeychains = [...keychains.matchAll(/^\s*"(.+)"\s*$/gm)].map(match => match[1]);
    run('security', ['create-keychain', '-p', keychainPassword, keychainPath]);
    run('security', ['unlock-keychain', '-p', keychainPassword, keychainPath]);
    run('security', ['set-keychain-settings', '-lut', '21600', keychainPath]);
    // codesign resolves certificate chains through the search list even with --keychain.
    run('security', ['list-keychains', '-d', 'user', '-s', keychainPath, ...originalKeychains]);
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
    identity = findIdentity();
    if (!identity && env.GITHUB_ACTIONS === 'true' && env.RUNNER_ENVIRONMENT === 'github-hosted') {
      // Trust the existing self-signed identity only on a disposable hosted runner.
      const certificate = run('security', [
        'find-certificate', '-c', SIGNING_IDENTITY, '-p', keychainPath,
      ], { encoding: 'utf8', stdio: 'pipe' }).stdout;
      writeFileSync(publicCertificate, certificate);
      temporaryTrust = true;
      run('sudo', ['security', 'add-trusted-cert', '-d', '-r', 'trustRoot',
        '-p', 'codeSign', '-k', keychainPath, publicCertificate]);
      identity = findIdentity();
    }
    if (!identity) throw new Error(`No valid ${SIGNING_IDENTITY} identity; refusing ad-hoc signing.`);
    const probe = path.join(temporaryDirectory, 'signing-probe');
    copyFileSync(process.execPath, probe);
    run('codesign', ['--force', '--sign', SIGNING_IDENTITY, '--keychain', keychainPath,
      '--timestamp=none', probe]);
    run('codesign', ['--verify', '--strict', probe]);
    console.log('[alpha-package] Signing preflight passed.');
  } catch (error) {
    cleanup();
    throw error;
  }

  return {
    environment: {
      ...env,
      CSC_IDENTITY_AUTO_DISCOVERY: 'true',
      CSC_KEYCHAIN: keychainPath,
      CSC_NAME: identity,
      PICTO_MAC_SIGNING_IDENTITY_HASH: identity,
    },
    cleanup,
  };
}

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
