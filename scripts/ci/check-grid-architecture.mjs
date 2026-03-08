/**
 * Guard: grid architecture invariants.
 *
 * Scans src/features/grid/ for violations of the grid architecture
 * established in PBIs 151–156:
 *
 * 1. GridController.fetchGridPage only in GridQueryBroker.ts
 * 2. Transition dispatch (BEGIN_FADE_OUT, COMMIT_TRANSITION, ABORT_TRANSITION,
 *    END_FADE) only in ImageGrid.tsx, useGridTransitionController.ts, and gridRuntimeReducer.ts
 * 3. COMMIT_GEOMETRY dispatch only in ImageGrid.tsx, useGridTransitionController.ts, and gridRuntimeReducer.ts
 * 4. new GridQueryBroker only in useGridQueryBroker.ts
 *
 * Lines containing `guard:allow` are exempted.
 */

import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const gridDir = path.join(root, 'src', 'features', 'grid');

// ── File walker ────────────────────────────────────────────────

function walk(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx|js|jsx)$/.test(entry.name)) out.push(p);
  }
  return out;
}

// ── Rules ──────────────────────────────────────────────────────

const GUARD_ALLOW = /guard:allow/;
const FETCH_GRID_PAGE_ALLOWED = new Set(['GridQueryBroker.ts', 'DetailView.tsx']);
const TRANSITION_DISPATCH_ALLOWED = new Set([
  'ImageGrid.tsx',
  'useGridTransitionController.ts',
  'gridRuntimeReducer.ts',
]);
const COMMIT_GEOMETRY_ALLOWED = new Set([
  'ImageGrid.tsx',
  'useGridTransitionController.ts',
  'gridRuntimeReducer.ts',
]);

/**
 * @typedef {{ file: string, line: number, text: string, rule: string }} Violation
 */

/**
 * @param {string} filePath
 * @param {string} fileName
 * @returns {Violation[]}
 */
function checkFile(filePath, fileName) {
  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split('\n');
  /** @type {Violation[]} */
  const violations = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (GUARD_ALLOW.test(line)) continue;

    // Rule 1: GridController.fetchGridPage only in approved query owners
    if (
      /GridController\.fetchGridPage/.test(line) &&
      !FETCH_GRID_PAGE_ALLOWED.has(fileName)
    ) {
      violations.push({
        file: filePath,
        line: i + 1,
        text: line.trim(),
        rule: 'GridController.fetchGridPage must only be called in GridQueryBroker.ts or DetailView.tsx',
      });
    }

    // Rule 2: Transition dispatch only in approved transition owners
    if (
      /type:\s*['"](?:BEGIN_FADE_OUT|COMMIT_TRANSITION|ABORT_TRANSITION|END_FADE)['"]/.test(line) &&
      !TRANSITION_DISPATCH_ALLOWED.has(fileName)
    ) {
      violations.push({
        file: filePath,
        line: i + 1,
        text: line.trim(),
        rule: 'Transition dispatch (BEGIN_FADE_OUT/COMMIT_TRANSITION/ABORT_TRANSITION/END_FADE) only allowed in ImageGrid.tsx, useGridTransitionController.ts, and gridRuntimeReducer.ts',
      });
    }

    // Rule 3: COMMIT_GEOMETRY dispatch only in approved transition owners
    if (
      /type:\s*['"]COMMIT_GEOMETRY['"]/.test(line) &&
      !COMMIT_GEOMETRY_ALLOWED.has(fileName)
    ) {
      violations.push({
        file: filePath,
        line: i + 1,
        text: line.trim(),
        rule: 'COMMIT_GEOMETRY dispatch only allowed in ImageGrid.tsx, useGridTransitionController.ts, and gridRuntimeReducer.ts',
      });
    }

    // Rule 4: new GridQueryBroker only in useGridQueryBroker.ts
    if (
      /new\s+GridQueryBroker/.test(line) &&
      fileName !== 'useGridQueryBroker.ts'
    ) {
      violations.push({
        file: filePath,
        line: i + 1,
        text: line.trim(),
        rule: 'new GridQueryBroker() only allowed in useGridQueryBroker.ts',
      });
    }
  }

  return violations;
}

// ── Main ───────────────────────────────────────────────────────

if (!fs.existsSync(gridDir)) {
  console.error(`Grid directory not found: ${gridDir}`);
  process.exit(1);
}

const files = walk(gridDir);
let totalViolations = 0;

for (const filePath of files) {
  const fileName = path.basename(filePath);
  const violations = checkFile(filePath, fileName);
  if (violations.length === 0) continue;

  totalViolations += violations.length;
  const rel = path.relative(root, filePath);
  for (const v of violations) {
    console.error(`  FAIL ${rel}:${v.line}`);
    console.error(`    ${v.text}`);
    console.error(`    Rule: ${v.rule}`);
  }
}

if (totalViolations > 0) {
  console.error(`\nFAILED: ${totalViolations} grid architecture violation(s).`);
  console.error('If a violation is intentional, add // guard:allow — reason to the line.');
  process.exit(1);
} else {
  console.log('OK: grid architecture invariants hold.');
}
