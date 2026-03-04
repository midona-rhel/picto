import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const srcDir = path.join(root, 'src');

function walk(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx|js|jsx)$/.test(entry.name)) out.push(p);
  }
  return out;
}

const files = walk(srcDir);
const legacyVendor = `${String.fromCharCode(116, 97, 117, 114, 105)}-apps`;
const bannedImportPrefix = `@${legacyVendor}/`;
const offenders = [];
for (const file of files) {
  const txt = fs.readFileSync(file, 'utf8');
  if (txt.includes(bannedImportPrefix)) offenders.push(file);
}

if (offenders.length > 0) {
  console.error('Found forbidden legacy runtime imports:');
  for (const file of offenders) console.error(` - ${path.relative(root, file)}`);
  process.exit(1);
}

console.log('OK: no forbidden legacy runtime imports in src/.');
