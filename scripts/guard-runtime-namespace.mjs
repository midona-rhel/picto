#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const scanDirs = [
  'core/src',
  'electron',
  'native/picto-node/src',
  'src',
].map((p) => path.join(root, p));

const scanFiles = [
  path.join(root, 'package.json'),
  path.join(root, 'core/Cargo.toml'),
  path.join(root, 'native/picto-node/Cargo.toml'),
];

const sourceExt = /\.(ts|tsx|js|jsx|mjs|cjs|rs|toml|json)$/;
const banned = /\bimaginator\b/gi;

const allowlistedMatches = [
  {
    relPath: 'electron/globalConfig.mjs',
    pattern: /path\.join\(appData,\s*'imaginator',\s*'config\.json'\)/g,
    reason: 'one-time legacy config migration fallback',
  },
];

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (
      entry.name === 'node_modules' ||
      entry.name === 'dist' ||
      entry.name === 'target' ||
      entry.name === '.git'
    ) {
      continue;
    }
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

function isAllowedMatch(relPath, content, index) {
  const rules = allowlistedMatches.filter((rule) => rule.relPath === relPath);
  if (rules.length === 0) return false;
  for (const rule of rules) {
    const re = new RegExp(rule.pattern.source, rule.pattern.flags);
    let match;
    while ((match = re.exec(content)) !== null) {
      const start = match.index;
      const end = start + match[0].length;
      if (index >= start && index < end) return true;
    }
  }
  return false;
}

const files = [...new Set([...scanDirs.flatMap(walk), ...scanFiles.filter((f) => fs.existsSync(f))])];
const offenders = [];

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  banned.lastIndex = 0;
  let match;
  while ((match = banned.exec(content)) !== null) {
    const relPath = path.relative(root, file);
    if (isAllowedMatch(relPath, content, match.index)) continue;
    offenders.push({
      file: relPath,
      line: lineForIndex(content, match.index),
      match: match[0],
    });
  }
}

if (offenders.length > 0) {
  console.error("Found forbidden runtime namespace token 'imaginator':");
  for (const offender of offenders) {
    console.error(` - ${offender.file}:${offender.line} (${offender.match})`);
  }
  console.error("Use canonical runtime/product namespace 'picto'.");
  process.exit(1);
}

console.log("OK: runtime/product namespace is canonicalized to 'picto' in active paths.");
