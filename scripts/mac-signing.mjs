// Signs macOS app bundles with the project's own certificate, "Picto Code
// Signing", and checks the result. The Electron package
// (`build-alpha-package.mjs`, `verify-mac-package.mjs`) and the GPUI package
// (`sign-mac-app.mjs`) both use this module, so that there is one signing
// policy: a bundle is signed with this certificate or the build fails, and an
// ad-hoc signature is never accepted in its place.

import { X509Certificate, randomBytes } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';

export const SIGNING_IDENTITY = 'Picto Code Signing';

// Runs a command and throws when it fails. A command that has not ended
// after 30 seconds is killed, so that a `security` or `codesign` call that
// waits for a password dialog fails the build and does not hang it.
export function run(command, args, options = {}) {
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

// Returns the certificate file and its password, or null when there are
// none. `MAC_CSC_LINK` is the certificate, as a file path or as the file's
// contents in base64, and `MAC_CSC_KEY_PASSWORD` is its password. Without
// both variables, a developer's own certificate in the Picto signing folder
// of the per-user application data folder is used when it exists.
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

// Returns the SHA-1 fingerprint of the valid "Picto Code Signing" identity in
// the output of `security find-identity -v`, or null. An identity that the
// system lists as expired or not trusted has a remark after its name and is
// not matched.
export function signingIdentity(output) {
  return output.match(/^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"Picto Code Signing"\s*$/m)?.[1] ?? null;
}

// Imports the certificate into a temporary keychain and proves that it can
// sign. Returns null on other systems, and on macOS when there is no
// certificate and `PICTO_REQUIRE_MAC_SIGNING` is not `true`. Otherwise it
// returns the environment for the tool that signs (the keychain's path in
// `CSC_KEYCHAIN`, the identity's fingerprint in `CSC_NAME` and
// `PICTO_MAC_SIGNING_IDENTITY_HASH`) and a `cleanup` function, which the
// caller must call: it restores the user's keychain search list and deletes
// the temporary keychain.
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
  let originalKeychains = null;
  const cleanup = () => {
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
    // `codesign` looks for the certificate chain in the keychain search list,
    // even when it is given `--keychain`, so the temporary keychain is put
    // first in that list.
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
      // A self-signed certificate is valid for signing only once the system
      // trusts it. On a GitHub-hosted runner, which is thrown away after the
      // job, the certificate is trusted for code signing here. This is not
      // done on any other machine, because removing the trust again needs an
      // administrator's password. The cleanup still deletes the keychain that
      // holds the private key.
      const certificate = run('security', [
        'find-certificate', '-c', SIGNING_IDENTITY, '-p', keychainPath,
      ], { encoding: 'utf8', stdio: 'pipe' }).stdout;
      writeFileSync(publicCertificate, certificate);
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

// Checks that the app bundle at `app` is signed with the certificate whose
// SHA-1 fingerprint is `expectedFingerprint`, and that every program inside
// it is signed too (`codesign --verify --deep --strict`). With
// `requireIconCatalog`, the bundle must also hold a compiled Icon Composer
// catalog (`Contents/Resources/Assets.car`). Throws when a check fails.
export function verifyMacSignature(app, expectedFingerprint, { requireIconCatalog = false } = {}) {
  if (!/^[a-f0-9]{40}$/i.test(expectedFingerprint ?? '')) throw new Error('Missing expected macOS signing identity.');
  execFileSync('codesign', ['--verify', '--deep', '--strict', app], { stdio: 'pipe' });
  const scratch = mkdtempSync(path.join(tmpdir(), 'picto-signature-check-'));
  try {
    const prefix = path.join(scratch, 'certificate-');
    execFileSync('codesign', ['--display', `--extract-certificates=${prefix}`, app], { stdio: 'pipe' });
    const certificate = new X509Certificate(readFileSync(`${prefix}0`));
    if (certificate.fingerprint.replaceAll(':', '').toLowerCase() !== expectedFingerprint.toLowerCase()) {
      throw new Error('Packaged app does not use the configured macOS signing certificate.');
    }
    if (requireIconCatalog
      && statSync(path.join(app, 'Contents', 'Resources', 'Assets.car')).size === 0) {
      throw new Error('Packaged app is missing the compiled Icon Composer catalog.');
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
