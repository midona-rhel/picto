#!/usr/bin/env node

// Signs the macOS app bundle given as the first argument with the project's
// own certificate, "Picto Code Signing", and checks the result. The package
// script (`experiments/gpui/package.sh`) calls this before it makes the disk
// image and the archive.
//
// The certificate comes only from the environment variables `MAC_CSC_LINK`
// and `MAC_CSC_KEY_PASSWORD`. Without them the script signs nothing and
// fails: it never looks for a certificate on the machine.
//
// The bundle is signed with the certificate and without a timestamp. It is
// not notarized, because the project has no Apple Developer ID.

import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import path from 'node:path';
import { prepareMacSigning, verifyMacSignature } from './mac-signing.mjs';

const app = process.argv[2];
if (!app) {
  console.error('Usage: sign-mac-app.mjs <path to the .app bundle>');
  process.exit(2);
}
if (!process.env.MAC_CSC_LINK || !process.env.MAC_CSC_KEY_PASSWORD) {
  console.error('sign-mac-app: MAC_CSC_LINK and MAC_CSC_KEY_PASSWORD are not both set, so nothing was signed.');
  process.exit(1);
}

const requireIconCatalog = process.env.PICTO_REQUIRE_MAC_SIGNING === 'true';
const signing = prepareMacSigning({ ...process.env, PICTO_REQUIRE_MAC_SIGNING: 'true' });
try {
  const identity = signing.environment.PICTO_MAC_SIGNING_IDENTITY_HASH;
  const keychain = signing.environment.CSC_KEYCHAIN;
  const sign = (target) => execFileSync('codesign', [
    '--force', '--sign', identity, '--keychain', keychain, '--timestamp=none', target,
  ], { stdio: 'inherit' });
  // Every program inside the bundle is signed before the bundle itself,
  // because the bundle's signature records the signatures of what is inside.
  // The bundle's own program is signed together with the bundle.
  const programs = path.join(app, 'Contents', 'MacOS');
  const main = path.basename(app, '.app');
  for (const name of readdirSync(programs)) {
    if (name !== main) sign(path.join(programs, name));
  }
  sign(app);
  verifyMacSignature(app, identity, { requireIconCatalog });
  console.log(`sign-mac-app: ${app} is signed with the certificate "Picto Code Signing" (${identity}).`);
} finally {
  signing.cleanup();
}
