#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const SRC_DIR = path.join(ROOT, 'src');

const ALWAYS_ALLOWED_PREFIXES = [
  'src/controllers/',
  'src/platform/',
];

// Exact-file debt allowlist. New raw backend api imports must not be added
// outside controllers/platform; this list should shrink domain by domain.
const LEGACY_ALLOWED_FILES = new Set([
]);

const API_IMPORT_PATTERNS = [
  // Raw api object
  /import\s*\{[^}]*\bapi\b[^}]*\}\s*from\s*['"]#desktop\/api['"]/g,
  /import\s*\{[^}]*\bapi\b[^}]*\}\s*from\s*['"][^'"]*platform\/api['"]/g,
  // queryApi / commandApi from barrel re-export
  /import\s*\{[^}]*\bqueryApi\b[^}]*\}\s*from\s*['"]#desktop\/api['"]/g,
  /import\s*\{[^}]*\bqueryApi\b[^}]*\}\s*from\s*['"][^'"]*platform\/api['"]/g,
  /import\s*\{[^}]*\bcommandApi\b[^}]*\}\s*from\s*['"]#desktop\/api['"]/g,
  /import\s*\{[^}]*\bcommandApi\b[^}]*\}\s*from\s*['"][^'"]*platform\/api['"]/g,
  // queryApi / commandApi from their own modules
  /import\b.*from\s*['"]#desktop\/queryApi['"]/g,
  /import\b.*from\s*['"][^'"]*platform\/queryApi['"]/g,
  /import\b.*from\s*['"]#desktop\/commandApi['"]/g,
  /import\b.*from\s*['"][^'"]*platform\/commandApi['"]/g,
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

function isAlwaysAllowed(file) {
  const rel = toRelative(file);
  return ALWAYS_ALLOWED_PREFIXES.some((prefix) => rel.startsWith(prefix));
}

function isLegacyAllowed(file) {
  return LEGACY_ALLOWED_FILES.has(toRelative(file));
}

async function main() {
  const files = await walk(SRC_DIR);
  const violations = [];

  for (const file of files) {
    if (isAlwaysAllowed(file) || isLegacyAllowed(file)) continue;
    const content = await fs.readFile(file, 'utf8');
    for (const pattern of API_IMPORT_PATTERNS) {
      if (pattern.test(content)) {
        violations.push(toRelative(file));
        break;
      }
      pattern.lastIndex = 0;
    }
  }

  if (violations.length > 0) {
    console.error('Backend access boundary check FAILED.\n');
    console.error('Raw backend api imports are only allowed in controllers/platform or the explicit legacy allowlist.\n');
    for (const violation of violations) {
      console.error(`- ${violation}`);
    }
    process.exit(1);
  }

  console.log(`Backend access boundary check passed. Legacy allowlist: ${LEGACY_ALLOWED_FILES.size} file(s).`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
