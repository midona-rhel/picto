#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const TARGET_DIRS = ['src/components', 'src/hooks'];
const EXTENSIONS = new Set(['.ts', '.tsx']);
const VIOLATION_RE = /\bSidebarController\.requestRefresh\s*\(/;
const GUARD_ALLOW_RE = /guard:allow/;

async function walk(dir, out) {
  let entries;
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.name.startsWith('.')) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'dist') continue;
      await walk(full, out);
      continue;
    }
    if (!EXTENSIONS.has(path.extname(entry.name))) continue;
    out.push(full);
  }
}

async function main() {
  const files = [];
  for (const rel of TARGET_DIRS) {
    await walk(path.join(ROOT, rel), files);
  }

  const violations = [];
  for (const file of files) {
    const text = await fs.readFile(file, 'utf8');
    const lines = text.split(/\r\n|\n|\r/);
    lines.forEach((line, idx) => {
      if (!VIOLATION_RE.test(line)) return;
      if (GUARD_ALLOW_RE.test(line)) return;
      const rel = path.relative(ROOT, file).replaceAll(path.sep, '/');
      violations.push(`${rel}:${idx + 1} -> ${line.trim()}`);
    });
  }

  if (violations.length > 0) {
    console.error('UI sidebar refresh guard FAILED.\n');
    console.error('Move sidebar refresh fanout into shared domain action/effect modules.');
    console.error('If a line must remain, add an inline `guard:allow` comment with reason.\n');
    for (const v of violations) console.error(`- ${v}`);
    process.exit(1);
  }

  console.log('UI sidebar refresh guard passed.');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
