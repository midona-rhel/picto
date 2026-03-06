#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const srcDir = path.join(root, 'src');
const sourceExt = /\.(ts|tsx)$/;

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...walk(full));
      continue;
    }
    if (sourceExt.test(entry.name)) out.push(full);
  }
  return out;
}

function lineForIndex(content, index) {
  let line = 1;
  for (let i = 0; i < index; i += 1) {
    if (content.charCodeAt(i) === 10) line += 1;
  }
  return line;
}

function shouldScan(relPath) {
  if (relPath.startsWith('src/components/')) return false;
  if (relPath.startsWith('src/features/')) return false;
  if (relPath.startsWith('src/test/')) return false;
  return true;
}

function isLegacyComponentImport(specifier) {
  const normalized = specifier.replaceAll('\\', '/');
  const mentionsComponents = normalized.includes('/components/')
    || normalized.startsWith('./components/')
    || normalized.startsWith('../components/')
    || normalized.startsWith('../../components/')
    || normalized.startsWith('../../../components/');
  if (!mentionsComponents) return false;

  // Shared presentation primitives remain in src/components/ui.
  if (normalized.includes('/components/ui/')) return false;
  if (normalized.startsWith('#ui/')) return false;

  return true;
}

const files = walk(srcDir);
const importRe = /(from\s+['"]([^'"]+)['"])|(import\(\s*['"]([^'"]+)['"]\s*\))/g;
const offenders = [];

for (const file of files) {
  const relPath = path.relative(root, file).replaceAll('\\', '/');
  if (!shouldScan(relPath)) continue;
  const content = fs.readFileSync(file, 'utf8');
  let match;
  while ((match = importRe.exec(content)) !== null) {
    const specifier = match[2] ?? match[4];
    if (!specifier) continue;
    if (!isLegacyComponentImport(specifier)) continue;
    offenders.push({
      file: relPath,
      line: lineForIndex(content, match.index),
      specifier,
    });
  }
}

if (offenders.length > 0) {
  console.error('Found direct legacy component imports outside feature/component layers:');
  for (const offender of offenders) {
    console.error(` - ${offender.file}:${offender.line} -> ${offender.specifier}`);
  }
  console.error('Import via #features/* facades (or #ui/* for shared primitives).');
  process.exit(1);
}

console.log('OK: non-feature layers import domain UI via feature facades.');
