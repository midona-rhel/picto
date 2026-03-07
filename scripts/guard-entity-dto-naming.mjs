#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const scanDirs = [
  path.join(root, 'src/components/image-grid'),
  path.join(root, 'src/controllers'),
];
const scanFiles = [
  path.join(root, 'src/platform/api.ts'),
];
const sourceExt = /\.(ts|tsx)$/;

const bannedTokens = [
  'ImageItem',
  'FileAllMetadata',
  'FileMetadataBatchResponse',
];

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (sourceExt.test(entry.name)) out.push(full);
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

const files = [...new Set([...scanDirs.flatMap(walk), ...scanFiles.filter((f) => fs.existsSync(f))])];
const offenders = [];

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  const relPath = path.relative(root, file);
  for (const token of bannedTokens) {
    const re = new RegExp(`\\b${token}\\b`, 'g');
    let match;
    while ((match = re.exec(content)) !== null) {
      offenders.push({
        file: relPath,
        line: lineForIndex(content, match.index),
        token,
      });
    }
  }
}

if (offenders.length > 0) {
  console.error('Found legacy file/image DTO names in entity-facing surfaces:');
  for (const offender of offenders) {
    console.error(` - ${offender.file}:${offender.line} (${offender.token})`);
  }
  console.error('Use canonical DTO names: EntitySlim, EntityDetails, EntityMetadataBatchResponse.');
  process.exit(1);
}

console.log('OK: entity-facing DTO naming is canonical in grid/controllers/api surfaces.');
