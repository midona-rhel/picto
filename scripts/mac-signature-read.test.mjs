import { expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { X509Certificate } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { verifyMacSignature } from './mac-signing.mjs';

it.skipIf(process.platform !== 'darwin')('reads a real macOS signature without modifying the signed app', () => {
  const finder = '/System/Library/CoreServices/Finder.app';
  const scratch = mkdtempSync(path.join(tmpdir(), 'picto-certificate-test-'));
  try {
    const prefix = path.join(scratch, 'certificate-');
    execFileSync('codesign', ['--display', `--extract-certificates=${prefix}`, finder]);
    const certificate = new X509Certificate(readFileSync(`${prefix}0`));
    expect(() => verifyMacSignature(finder, certificate.fingerprint.replaceAll(':', ''))).not.toThrow();
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
