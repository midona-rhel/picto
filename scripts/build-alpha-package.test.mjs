import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync, execFileSync } from 'node:child_process';
import { packageArguments, prepareMacSigning, signingIdentity, supportsIconComposer } from './build-alpha-package.mjs';
import verifyMacPackage from './verify-mac-package.mjs';

const state = vi.hoisted(() => ({ fingerprint: 'AB'.repeat(20) }));
vi.mock('node:child_process', () => {
  const mock = { spawnSync: vi.fn(), execFileSync: vi.fn() };
  return { ...mock, default: mock };
});
vi.mock('node:crypto', async (original) => {
  const mock = { ...await original(), X509Certificate: class { fingerprint = state.fingerprint; } };
  return { ...mock, default: mock };
});
vi.mock('node:fs', async (original) => {
  const mock = { ...await original(), copyFileSync: vi.fn() };
  return { ...mock, default: mock };
});
const hash = 'AB'.repeat(20);
const valid = `  1) ${hash} "Picto Code Signing"\n     1 valid identities found`;
const credentials = { MAC_CSC_LINK: 'ZmFrZQ==', MAC_CSC_KEY_PASSWORD: 'fixture-password', PICTO_REQUIRE_MAC_SIGNING: 'true' };
const scratch = [];
beforeEach(() => {
  vi.resetAllMocks();
  spawnSync.mockReturnValue({ status: 0, stdout: valid });
  state.fingerprint = hash;
});
afterEach(() => {
  vi.unstubAllEnvs();
  for (const dir of scratch.splice(0)) rmSync(dir, { recursive: true, force: true });
});

it('accepts only the configured valid identity, not expired/untrusted identities or a different name', () => {
  expect(signingIdentity(valid)).toBe(hash);
  expect(signingIdentity(`${valid.split('\n')[0]} (CSSMERR_TP_NOT_TRUSTED)`)).toBeNull();
  expect(signingIdentity(valid.replace('Picto Code Signing', 'Another Identity'))).toBeNull();
  expect(signingIdentity('0 valid identities found')).toBeNull();
});

it('cleans its temporary keychain and fails before packaging when local credentials have no valid identity', () => {
  spawnSync.mockReturnValue({ status: 0, stdout: '0 valid identities found' });
  expect(() => prepareMacSigning(credentials, 'darwin')).toThrow('refusing ad-hoc');
  const creation = spawnSync.mock.calls.find(([c, a]) => c === 'security' && a[0] === 'create-keychain');
  expect(existsSync(path.dirname(creation[1].at(-1)))).toBe(false);
  expect(spawnSync.mock.calls.some(([c]) => c === 'sudo')).toBe(false);
});

it('passes the exact configured certificate fingerprint and cleans up after a valid import', () => {
  const signing = prepareMacSigning(credentials, 'darwin');
  expect(signing.environment.CSC_NAME).toBe(hash);
  expect(signing.environment.PICTO_MAC_SIGNING_IDENTITY_HASH).toBe(hash);
  const keychain = signing.environment.CSC_KEYCHAIN;
  signing.cleanup();
  expect(existsSync(path.dirname(keychain))).toBe(false);
  expect(spawnSync.mock.calls.some(([c]) => c === 'sudo')).toBe(false);
});

it('keeps existing keychains searchable and restores the exact search list after signing', () => {
  const existing = ['/fixture/login.keychain-db', '/fixture/another.keychain-db'];
  spawnSync.mockImplementation((command, args) => ({ status: 0, stdout:
    args[0] === 'list-keychains' && args.length === 3
      ? existing.map(p => `    "${p}"`).join('\n') : valid,
  }));
  const signing = prepareMacSigning(credentials, 'darwin');
  const keychain = signing.environment.CSC_KEYCHAIN;
  expect(spawnSync).toHaveBeenCalledWith('security', ['list-keychains', '-d', 'user', '-s', keychain, ...existing], expect.anything());
  expect(spawnSync).toHaveBeenCalledWith('codesign', ['--force', '--sign', 'Picto Code Signing', '--keychain', keychain,
    '--timestamp=none', path.join(path.dirname(keychain), 'signing-probe')], expect.objectContaining({ timeout: 30_000, killSignal: 'SIGKILL' }));
  signing.cleanup();
  expect(spawnSync).toHaveBeenCalledWith('security', ['list-keychains', '-d', 'user', '-s', ...existing], expect.anything());
});

it('fails before packaging and restores the keychain list when actual signing times out', () => {
  spawnSync.mockImplementation((command, args) => command === 'codesign'
    ? { status: null, error: { code: 'ETIMEDOUT' } }
    : { status: 0, stdout: args[0] === 'list-keychains' ? '"/fixture/login.keychain-db"' : valid });
  expect(() => prepareMacSigning(credentials, 'darwin')).toThrow('codesign --force failed: ETIMEDOUT');
  expect(spawnSync).toHaveBeenCalledWith('security', ['list-keychains', '-d', 'user', '-s', '/fixture/login.keychain-db'], expect.anything());
  const creation = spawnSync.mock.calls.find(([c, a]) => c === 'security' && a[0] === 'create-keychain');
  expect(existsSync(path.dirname(creation[1].at(-1)))).toBe(false);
});

it('still restores the keychain search list and deletes scratch data if temporary trust removal fails', () => {
  let reads = 0;
  spawnSync.mockImplementation((command, args) => {
    if (command === 'sudo' && args[1] === 'remove-trusted-cert') return { status: null, error: { code: 'ETIMEDOUT' } };
    return { status: 0, stdout: args[0] === 'find-identity' && ++reads === 1 ? '0 valid identities found' : valid };
  });
  const signing = prepareMacSigning({ ...credentials, GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }, 'darwin');
  expect(() => signing.cleanup()).toThrow('ETIMEDOUT');
  expect(existsSync(path.dirname(signing.environment.CSC_KEYCHAIN))).toBe(false);
  expect(spawnSync.mock.calls.some(([, a]) => a[0] === 'list-keychains' && a.length === 4)).toBe(true);
});

it('temporarily trusts the self-signed certificate only on disposable GitHub-hosted runners', () => {
  let reads = 0;
  spawnSync.mockImplementation((command, args) => ({ status: 0, stdout:
    command === 'security' && args[0] === 'find-identity'
      ? (++reads === 1 ? '0 valid identities found' : valid) : 'public certificate',
  }));
  const signing = prepareMacSigning({ ...credentials, GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }, 'darwin');
  expect(signing.environment.CSC_NAME).toBe(hash);
  signing.cleanup();
  expect(spawnSync.mock.calls.filter(([c]) => c === 'sudo').map(([, a]) => a[1]))
    .toEqual(['add-trusted-cert', 'remove-trusted-cert']);
});

it('does not modify trust on self-hosted CI runners', () => {
  spawnSync.mockReturnValue({ status: 0, stdout: '0 valid identities found' });
  expect(() => prepareMacSigning({ ...credentials, GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'self-hosted' }, 'darwin')).toThrow('refusing ad-hoc');
  expect(spawnSync.mock.calls.some(([c]) => c === 'sudo')).toBe(false);
});

it('cleans temporary trust even if the certificate remains invalid', () => {
  spawnSync.mockReturnValue({ status: 0, stdout: '0 valid identities found' });
  expect(() => prepareMacSigning({ ...credentials, GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }, 'darwin')).toThrow('refusing ad-hoc');
  expect(spawnSync.mock.calls.some(([c, a]) => c === 'sudo' && a[1] === 'remove-trusted-cert')).toBe(true);
});

it('uses the Xcode marketing version and requires actool instead of parsing its unrelated version format', () => {
  spawnSync.mockImplementation(c => ({ status: 0, stdout: c === 'xcodebuild' ? 'Xcode 26.6\nBuild version 17G500' : '' }));
  expect(supportsIconComposer('darwin')).toBe(true);
  spawnSync.mockReturnValue({ status: 0, stdout: 'Xcode 16.4\nBuild version 16F6' });
  expect(supportsIconComposer('darwin')).toBe(false);
  spawnSync.mockImplementation(c => ({ status: c === 'xcrun' ? 1 : 0, stdout: 'Xcode 26.6' }));
  expect(supportsIconComposer('darwin')).toBe(false);
});

it('requires the release icon and forces signing without changing Windows or Linux packaging', () => {
  const signing = { environment: { PICTO_MAC_SIGNING_IDENTITY_HASH: hash } };
  expect(() => packageArguments({ platform: 'darwin', env: credentials, iconComposer: false, signing })).toThrow('requires Xcode 26');
  const args = packageArguments({ platform: 'darwin', env: credentials, iconComposer: true, signing });
  expect(args).toContain('--config.forceCodeSigning=true');
  expect(args).toContain(`--config.mac.identity=${hash}`);
  expect(args.some(arg => arg.includes('verify-mac-package.mjs'))).toBe(true);
  expect(args.some(arg => arg.includes('icon-flat'))).toBe(false);
  for (const platform of ['win32', 'linux']) {
    expect(prepareMacSigning(credentials, platform)).toBeNull();
    expect(packageArguments({ platform, env: {}, iconComposer: true, signing: null })).toHaveLength(2);
  }
});

describe('final packaged app check', () => {
  function fixture() {
    const dir = mkdtempSync(path.join(tmpdir(), 'picto-package-test-'));
    scratch.push(dir);
    mkdirSync(path.join(dir, 'Picto.app/Contents/Resources'), { recursive: true });
    writeFileSync(path.join(dir, 'Picto.app/Contents/Resources/Assets.car'), 'compiled catalog');
    vi.stubEnv('PICTO_MAC_SIGNING_IDENTITY_HASH', hash);
    vi.stubEnv('PICTO_REQUIRE_MAC_SIGNING', 'true');
    execFileSync.mockImplementation((c, args) => {
      if (args[1] === '--extract-certificates') writeFileSync(`${args[2]}0`, 'DER fixture');
    });
    return { electronPlatformName: 'darwin', appOutDir: dir, packager: { appInfo: { productFilename: 'Picto' } } };
  }
  it('verifies the app deeply, matches its leaf certificate and requires the compiled icon before archives', () => {
    const context = fixture();
    expect(() => verifyMacPackage(context)).not.toThrow();
    expect(execFileSync).toHaveBeenCalledWith('codesign', ['--verify', '--deep', '--strict', path.join(context.appOutDir, 'Picto.app')], { stdio: 'pipe' });
  });
  it('rejects an unrelated signature', () => {
    const context = fixture(); state.fingerprint = 'CD'.repeat(20);
    expect(() => verifyMacPackage(context)).toThrow('configured macOS signing certificate');
  });
  it('rejects an ad-hoc signature without a certificate', () => {
    const context = fixture(); execFileSync.mockImplementation(() => {});
    expect(() => verifyMacPackage(context)).toThrow();
  });
  it('rejects a missing Icon Composer catalog', () => {
    const context = fixture(); rmSync(path.join(context.appOutDir, 'Picto.app/Contents/Resources/Assets.car'));
    expect(() => verifyMacPackage(context)).toThrow();
  });
});
