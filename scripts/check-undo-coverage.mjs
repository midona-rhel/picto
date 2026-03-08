#!/usr/bin/env node
/**
 * Undo/redo coverage checker.
 *
 * Goal: fail CI if key user-action surfaces lose undo wiring.
 * This is intentionally static and conservative: it does not try to
 * prove full behavioral correctness, but it does catch accidental
 * regressions where undo hooks are removed from core UI domains.
 */
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();

function abs(file) {
  return path.join(ROOT, file);
}

async function read(file) {
  return fs.readFile(abs(file), 'utf8');
}

function countMatches(text, re) {
  const flags = re.flags.includes('g') ? re.flags : `${re.flags}g`;
  const globalRe = new RegExp(re.source, flags);
  let count = 0;
  while (globalRe.exec(text)) count += 1;
  return count;
}

const DOMAIN_MIN_COUNTS = [
  { file: 'src/features/grid/ImageGrid.tsx', min: 1 },
  { file: 'src/features/grid/hooks/useGridMutationActions.ts', min: 7 },
  { file: 'src/features/grid/hooks/useGridItemActions.ts', min: 1 },
  { file: 'src/features/tags/components/TagManager.tsx', min: 5 },
  { file: 'src/features/sidebar/hooks/useFolderTreeActions.ts', min: 10 },
  { file: 'src/features/sidebar/hooks/useFolderTreeDnd.ts', min: 1 },
  { file: 'src/features/sidebar/components/SmartFolderList.tsx', min: 4 },
  { file: 'src/features/smart-folders/components/SmartFolderModal.tsx', min: 2 },
  { file: 'src/features/settings/components/DuplicatesPanel.tsx', min: 1 },
  { file: 'src/features/duplicates/components/DuplicateManager.tsx', min: 1 },
];

const ROUTING_CHECKS = [
  {
    file: 'src/app/useNativeEventListeners.ts',
    checks: [
      /listen\('menu:undo'/,
      /listen\('menu:redo'/,
      /performUndo\(/,
      /performRedo\(/,
    ],
  },
  {
    file: 'electron/windows/menu.mjs',
    checks: [
      /label:\s*'Undo'/,
      /label:\s*'Redo'/,
      /menu:undo/,
      /menu:redo/,
    ],
  },
  {
    file: 'src/shared/lib/shortcuts.ts',
    checks: [
      /id:\s*'edit\.undo'/,
      /id:\s*'edit\.redo'/,
    ],
  },
];

async function main() {
  const errors = [];

  for (const item of DOMAIN_MIN_COUNTS) {
    const text = await read(item.file);
    const found = countMatches(text, /registerUndoAction\s*\(/g);
    if (found < item.min) {
      errors.push(
        `${item.file}: expected >= ${item.min} registerUndoAction() calls, found ${found}`,
      );
    }
  }

  for (const item of ROUTING_CHECKS) {
    const text = await read(item.file);
    for (const re of item.checks) {
      if (!re.test(text)) {
        errors.push(`${item.file}: missing required pattern ${re}`);
      }
    }
  }

  if (errors.length > 0) {
    console.error('Undo coverage check FAILED:\n');
    for (const err of errors) console.error(`- ${err}`);
    process.exit(1);
  }

  console.log('Undo coverage check passed.');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});

