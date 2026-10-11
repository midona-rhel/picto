import { afterEach, expect, it, vi } from 'vitest';
import { execFileSync } from 'node:child_process';
import { X509Certificate } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import verifyMacPackage from './verify-mac-package.mjs';

afterEach(() => vi.unstubAllEnvs());

it.skipIf(process.platform !== 'darwin')('reads a real macOS signature without modifying the signed app', () => {
  const scratch = mkdtempSync(path.join(tmpdir(), 'picto-certificate-test-'));
  try {
    const prefix = path.join(scratch, 'certificate-');
    execFileSync('codesign', ['--display', `--extract-certificates=${prefix}`, '/System/Library/CoreServices/Finder.app']);
    const certificate = new X509Certificate(readFileSync(`${prefix}0`));
    vi.stubEnv('PICTO_MAC_SIGNING_IDENTITY_HASH', certificate.fingerprint.replaceAll(':', ''));
    vi.stubEnv('PICTO_REQUIRE_MAC_SIGNING', 'false');
    expect(() => verifyMacPackage({ electronPlatformName: 'darwin', appOutDir: '/System/Library/CoreServices',
      packager: { appInfo: { productFilename: 'Finder' } } })).not.toThrow();
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
