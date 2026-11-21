import path from 'node:path';
import { verifyMacSignature } from './mac-signing.mjs';

// electron-builder calls this function after it has signed the macOS app and
// before it makes the disk image and the archive, while the signing keychain
// is still open. It checks the signature (`verifyMacSignature`).
export default function verifyMacPackage(context) {
  if (context.electronPlatformName !== 'darwin') return;
  const app = path.join(context.appOutDir, `${context.packager.appInfo.productFilename}.app`);
  const requireIconCatalog = process.env.PICTO_REQUIRE_MAC_SIGNING === 'true';
  verifyMacSignature(app, process.env.PICTO_MAC_SIGNING_IDENTITY_HASH, { requireIconCatalog });
  console.log('[alpha-package] Verified macOS signing certificate.');
  if (requireIconCatalog) console.log('[alpha-package] Verified packaged Icon Composer catalog.');
}
