import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();

const scanDirs = ['electron', 'src', 'scripts'].map((d) => path.join(root, d));

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx|js|jsx|mjs|cjs)$/.test(entry.name)) out.push(p);
  }
  return out;
}

const files = scanDirs.flatMap(walk);

// Construct the forbidden prefix dynamically to avoid self-match.
const prefix = ['src', 'tauri'].join('-');
const banned = [
  `${prefix}/`,
  `${prefix}\\`,
];

const offenders = [];
for (const file of files) {
  const txt = fs.readFileSync(file, 'utf8');
  for (const needle of banned) {
    if (txt.includes(needle)) {
      offenders.push({ file, needle });
    }
  }
}

if (offenders.length > 0) {
  console.error(`Found forbidden ${prefix} path references:`);
  for (const { file, needle } of offenders) {
    console.error(` - ${path.relative(root, file)} contains '${needle}'`);
  }
  process.exit(1);
}

console.log(`OK: no ${prefix} path references in electron/, src/, scripts/.`);
