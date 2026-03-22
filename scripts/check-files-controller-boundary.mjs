#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const SRC_DIR = path.join(ROOT, 'src');
const ALLOWED_PREFIXES = [
  'src/controllers/',
  'src/platform/',
  'src/shared/types/generated/',
];
const FORBIDDEN_PATTERNS = [
  { label: 'api.file.*', regex: /\bapi\.file\./g },
  { label: 'api.files.*', regex: /\bapi\.files\./g },
  { label: 'api.grid.getEntitiesMetadataBatch', regex: /\bapi\.grid\.getEntitiesMetadataBatch\b/g },
  { label: 'api.grid.getPageSlim', regex: /\bapi\.grid\.getPageSlim\b/g },
  { label: 'api.duplicates.findSimilar', regex: /\bapi\.duplicates\.findSimilar\b/g },
  { label: 'api.selection.updateRating', regex: /\bapi\.selection\.updateRating\b/g },
  { label: 'api.selection.setSourceUrls', regex: /\bapi\.selection\.setSourceUrls\b/g },
  { label: 'api.selection.setNotes', regex: /\bapi\.selection\.setNotes\b/g },
  { label: '#desktop/queryApi import', regex: /from\s+['"]#desktop\/queryApi['"]/g },
  { label: '#desktop/commandApi import', regex: /from\s+['"]#desktop\/commandApi['"]/g },
];

async function walk(dir) {
  const results = [];
  const entries = await fs.readdir(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...await walk(fullPath));
      continue;
    }
    if (entry.name.endsWith('.ts') || entry.name.endsWith('.tsx')) {
      results.push(fullPath);
    }
  }
  return results;
}

function toRelative(file) {
  return path.relative(ROOT, file).replace(/\\/g, '/');
}

function isAllowed(file) {
  const rel = toRelative(file);
  return ALLOWED_PREFIXES.some((prefix) => rel.startsWith(prefix));
}

async function main() {
  const files = await walk(SRC_DIR);
  const violations = [];

  for (const file of files) {
    if (isAllowed(file)) continue;
    const content = await fs.readFile(file, 'utf8');
    for (const pattern of FORBIDDEN_PATTERNS) {
      if (pattern.regex.test(content)) {
        violations.push(`${toRelative(file)}: ${pattern.label}`);
      }
      pattern.regex.lastIndex = 0;
    }
  }

  if (violations.length > 0) {
    console.error('Files controller boundary check FAILED.\n');
    for (const violation of violations) {
      console.error(`- ${violation}`);
    }
    process.exit(1);
  }

  console.log('Files controller boundary check passed.');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
