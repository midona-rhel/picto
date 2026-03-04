import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();

function findBackendCoreFile(relativeCorePath) {
  const candidates = fs.readdirSync(root, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => path.join(root, d.name, 'src', 'core', relativeCorePath));

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

const backendMedia = findBackendCoreFile('media_protocol.rs');
const backendBlobs = findBackendCoreFile('blob_store.rs');

const files = [
  path.join(root, 'src/lib/mediaUrl.ts'),
  ...(backendMedia ? [backendMedia] : []),
  ...(backendBlobs ? [backendBlobs] : []),
];

const banned = [
  'media://localhost/file/<hash>?mime=',
  '`media://localhost/thumb/<hash>`',
  'original_path_legacy',
  'thumbnail_path_legacy',
  'falls back to extensionless',
];

let failed = false;
for (const file of files) {
  if (!fs.existsSync(file)) continue;
  const txt = fs.readFileSync(file, 'utf8');
  for (const needle of banned) {
    if (txt.includes(needle)) {
      console.error(`Forbidden legacy media marker '${needle}' found in ${path.relative(root, file)}`);
      failed = true;
    }
  }
}

if (failed) process.exit(1);
console.log('OK: no legacy media markers detected in strict media files.');
