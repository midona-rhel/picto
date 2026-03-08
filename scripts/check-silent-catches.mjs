#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs']);
const ALLOW_MARKERS = ['silent-catch-ok', 'best-effort-ok', 'intentional-noop-catch'];

const TARGET_PATHS = [
  'src/app-shell',
  'src/features/sidebar',
  'src/features/layout/components/WindowControls.tsx',
  'src/features/layout/components/SidebarJobStatus.tsx',
  'src/features/grid/DetailView.tsx',
  'src/features/grid/DetailWindow.tsx',
  'src/features/grid/QuickLook.tsx',
  'src/features/grid/hooks/useGridItemActions.ts',
  'src/features/settings/components/GeneralPanel.tsx',
  'src/features/settings/components/PtrPanel.tsx',
];

const EMPTY_PROMISE_CATCH_RE =
  /\.catch\s*\(\s*(?:\(\s*[^)]*\s*\)|[A-Za-z_$][\w$]*)?\s*=>\s*\{\s*(?:\/\/[^\n]*\n\s*|\/\*[\s\S]*?\*\/\s*)*\}\s*\)/gm;
const EMPTY_TRY_CATCH_RE =
  /(?<!\.)catch\s*(?:\(\s*[^)]*\s*\))?\s*\{\s*(?:\/\/[^\n]*\n\s*|\/\*[\s\S]*?\*\/\s*)*\}/gm;

function isFile(pathStat) {
  return pathStat.isFile();
}

function shouldSkipDir(name) {
  return name === 'node_modules' || name === 'dist' || name === 'target' || name.startsWith('.');
}

function lineNumberAt(content, index) {
  let line = 1;
  for (let i = 0; i < index; i++) {
    if (content.charCodeAt(i) === 10) line++;
  }
  return line;
}

function isAllowed(matchText) {
  return ALLOW_MARKERS.some((marker) => matchText.includes(marker));
}

async function collectFiles(targetPath, files) {
  const abs = path.join(ROOT, targetPath);
  let stat;
  try {
    stat = await fs.stat(abs);
  } catch {
    return;
  }

  if (isFile(stat)) {
    if (EXTENSIONS.has(path.extname(abs))) files.push(abs);
    return;
  }

  const entries = await fs.readdir(abs, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.isDirectory()) {
      if (shouldSkipDir(entry.name)) continue;
      await collectFiles(path.join(targetPath, entry.name), files);
      continue;
    }
    const full = path.join(abs, entry.name);
    if (EXTENSIONS.has(path.extname(full))) files.push(full);
  }
}

function scanMatches(content, rel, regex, type, out) {
  regex.lastIndex = 0;
  let match;
  while ((match = regex.exec(content)) !== null) {
    const text = match[0];
    if (isAllowed(text)) continue;
    out.push({
      file: rel,
      line: lineNumberAt(content, match.index),
      type,
    });
  }
}

async function main() {
  const files = [];
  for (const target of TARGET_PATHS) {
    await collectFiles(target, files);
  }

  const violations = [];
  for (const abs of files) {
    const rel = path.relative(ROOT, abs).replaceAll(path.sep, '/');
    const content = await fs.readFile(abs, 'utf8');
    scanMatches(content, rel, EMPTY_PROMISE_CATCH_RE, 'empty promise catch', violations);
    scanMatches(content, rel, EMPTY_TRY_CATCH_RE, 'empty try/catch', violations);
  }

  if (violations.length > 0) {
    console.error('Silent catch violations found in critical frontend paths:');
    for (const v of violations) {
      console.error(` - ${v.file}:${v.line} (${v.type})`);
    }
    console.error(
      '\nUse observable handling (`runCriticalAction`, `runBestEffort`, `notifyError`) or annotate intentional no-op catches with one of: ' +
      ALLOW_MARKERS.join(', '),
    );
    process.exit(1);
  }

  console.log('No silent catch violations found in critical frontend paths.');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
