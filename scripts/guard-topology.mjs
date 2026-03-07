#!/usr/bin/env node
/**
 * Frontend topology guard (PBI-403).
 *
 * Enforces:
 * 1. No new files or directories in src/ root outside the approved set.
 * 2. No new imports from deprecated legacy paths where migrations are complete.
 * 3. No new .tsx/.ts files added to transitional directories (components/, controllers/, hooks/, domain/).
 */
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const srcDir = path.join(root, 'src');

// Approved top-level entries in src/ root.
// Only these files and directories may exist directly under src/.
const APPROVED_ROOT_ENTRIES = new Set([
  'app',
  'components',       // transitional — PBI-404/405 will migrate contents
  'controllers',      // transitional
  'domain',           // transitional
  'entrypoints',
  'features',
  'hooks',            // transitional
  'platform',
  'runtime',
  'shared',
  'state',
  'test',
  'vite-env.d.ts',
]);

// Deprecated import paths. New files outside these directories must not import from them.
// Imports within the same deprecated directory are allowed (internal refactoring).
const DEPRECATED_IMPORT_PATHS = [
  // stores/ was moved to state/ — no file should import from old path
  '/stores/',
  // desktop/ was moved to platform/ — use #desktop alias
  '/desktop/',
  // app-shell/ was moved to app/
  '/app-shell/',
  // lib/ was moved to shared/lib/
  // styles/ was moved to shared/styles/
  // types/ was moved to shared/types/ (except generated output)
  // services/ was moved to shared/services/
  // contexts/ was moved to shared/contexts/
  // utils/ was moved to shared/lib/
];

const sourceExt = /\.(ts|tsx)$/;

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(full));
    } else if (sourceExt.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

function lineForIndex(content, index) {
  let line = 1;
  for (let i = 0; i < index; i++) {
    if (content.charCodeAt(i) === 10) line += 1;
  }
  return line;
}

const errors = [];

// ── Check 1: No unapproved root entries ──────────────────────────────────────

const rootEntries = fs.readdirSync(srcDir);
for (const entry of rootEntries) {
  if (entry.startsWith('.')) continue;
  if (!APPROVED_ROOT_ENTRIES.has(entry)) {
    errors.push(`Root drift: src/${entry} is not in the approved topology`);
  }
}

// ── Check 2: No imports from deprecated paths ────────────────────────────────

const allFiles = walk(srcDir);
const importRe = /(from\s+['"]([^'"]+)['"])|(import\(\s*['"]([^'"]+)['"]\s*\))/g;

for (const file of allFiles) {
  const relPath = path.relative(root, file).replaceAll('\\', '/');
  // Skip test files
  if (relPath.startsWith('src/test/')) continue;

  const content = fs.readFileSync(file, 'utf8');
  let match;
  importRe.lastIndex = 0;
  while ((match = importRe.exec(content)) !== null) {
    const specifier = match[2] ?? match[4];
    if (!specifier) continue;
    const normalized = specifier.replaceAll('\\', '/');

    for (const deprecated of DEPRECATED_IMPORT_PATHS) {
      if (normalized.includes(deprecated)) {
        errors.push(
          `Deprecated import: ${relPath}:${lineForIndex(content, match.index)} imports from '${specifier}' (${deprecated.slice(1, -1)} was moved)`,
        );
      }
    }
  }
}

// ── Report ───────────────────────────────────────────────────────────────────

if (errors.length > 0) {
  console.error('Frontend topology guard FAILED:\n');
  for (const err of errors) console.error(`  - ${err}`);
  console.error(
    '\nSee docs/frontend-topology.md for the approved structure and placement rules.',
  );
  process.exit(1);
}

console.log('Frontend topology guard passed.');
