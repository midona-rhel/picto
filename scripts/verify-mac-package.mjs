import { X509Certificate } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

// Runs before electron-builder creates the DMG and ZIP, while the signing keychain is open.
export default function verifyMacPackage(context) {
  if (context.electronPlatformName !== 'darwin') return;
  const expected = process.env.PICTO_MAC_SIGNING_IDENTITY_HASH;
  if (!/^[a-f0-9]{40}$/i.test(expected ?? '')) throw new Error('Missing expected macOS signing identity.');
  const app = path.join(context.appOutDir, `${context.packager.appInfo.productFilename}.app`);
  execFileSync('codesign', ['--verify', '--deep', '--strict', app], { stdio: 'pipe' });
  const scratch = mkdtempSync(path.join(tmpdir(), 'picto-signature-check-'));
  try {
    const prefix = path.join(scratch, 'certificate-');
    execFileSync('codesign', ['--display', `--extract-certificates=${prefix}`, app], { stdio: 'pipe' });
    const certificate = new X509Certificate(readFileSync(`${prefix}0`));
    if (certificate.fingerprint.replaceAll(':', '').toLowerCase() !== expected.toLowerCase()) {
      throw new Error('Packaged app does not use the configured macOS signing certificate.');
    }
    if (process.env.PICTO_REQUIRE_MAC_SIGNING === 'true'
      && statSync(path.join(app, 'Contents', 'Resources', 'Assets.car')).size === 0) {
      throw new Error('Packaged app is missing the compiled Icon Composer catalog.');
    }
    console.log('[alpha-package] Verified macOS signing certificate.');
    if (process.env.PICTO_REQUIRE_MAC_SIGNING === 'true') {
      console.log('[alpha-package] Verified packaged Icon Composer catalog.');
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}
