#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const SRC_DIR = path.join(ROOT, 'src');
const ALLOWED_FILES = new Set([
  'src/shared/types/backendState.ts',
]);
const ALLOWED_PREFIXES = [
  'src/shared/types/generated/',
];
const FORBIDDEN_PATTERNS = [
  {
    label: 'generated runtime-contract barrel import',
    regex: /from\s+['"][^'"]*generated\/runtime-contract['"]/g,
  },
  {
    label: 'direct generated runtime-contract type import',
    regex: /from\s+['"][^'"]*generated\/runtime-contract\/[^'"]+['"]/g,
  },
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
  if (ALLOWED_FILES.has(rel)) return true;
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
    console.error('Backend state import check FAILED.\n');
    for (const violation of violations) {
      console.error(`- ${violation}`);
    }
    console.error('\nImport backend-owned runtime types from the frontend boundary file src/shared/types/backendState.ts.');
    process.exit(1);
  }

  console.log('Backend state import check passed.');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
