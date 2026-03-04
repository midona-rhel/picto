/**
 * Guard: no raw color literals in CSS modules.
 *
 * Scans *.module.css files under src/ for hex colors (#xxx), rgb(), rgba(),
 * hsl(), hsla() literals and fails if any are found.
 *
 * Exemptions:
 *   - Lines containing `guard:allow` in a comment are skipped.
 *   - Lines inside url(), content:, or font-family: are skipped.
 *   - Allowed keywords: transparent, currentColor, inherit, none.
 */

import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const srcDir = path.join(root, 'src');

// Patterns that indicate raw color literals
const COLOR_PATTERNS = [
  // Hex colors: #fff, #ffffff, #ffffffaa — but not inside url() or content strings
  /(?<![&\w])#[0-9a-fA-F]{3,8}\b/,
  // rgb/rgba/hsl/hsla function calls
  /\brgba?\s*\(/,
  /\bhsla?\s*\(/,
];

// Lines matching these are false positives (data URIs, content strings, font stacks)
const ALLOWED_IN_LINE = /url\(|content:|font-family:/;

// Inline exemption comment
const GUARD_ALLOW_RE = /guard:allow/;

function scanFile(filePath) {
  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split('\n');
  const violations = [];
  let inComment = false;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Track block comments
    if (line.includes('/*')) inComment = true;
    if (line.includes('*/')) { inComment = false; continue; }
    if (inComment) continue;

    // Skip single-line comments
    const trimmed = line.trim();
    if (trimmed.startsWith('/*') || trimmed.startsWith('//')) continue;

    // Skip lines with url() or content: (may contain data URIs with hex)
    if (ALLOWED_IN_LINE.test(line)) continue;

    // Skip lines with inline guard:allow exemption
    if (GUARD_ALLOW_RE.test(line)) continue;

    for (const pattern of COLOR_PATTERNS) {
      if (pattern.test(line)) {
        violations.push({ line: i + 1, text: trimmed });
        break;
      }
    }
  }

  return violations;
}

// Find all module.css files under src/
function findModuleCssFiles(dir) {
  const results = [];
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules') continue;
      results.push(...findModuleCssFiles(full));
    } else if (entry.name.endsWith('.module.css')) {
      results.push(full);
    }
  }
  return results;
}

const files = findModuleCssFiles(srcDir);
let totalViolations = 0;

for (const file of files) {
  const rel = path.relative(root, file);
  const violations = scanFile(file);
  if (violations.length === 0) continue;

  totalViolations += violations.length;
  console.error(`  FAIL ${rel}:`);
  for (const v of violations) {
    console.error(`    L${v.line}: ${v.text}`);
  }
}

if (totalViolations > 0) {
  console.error(`\nFAILED: ${totalViolations} raw color literal(s) in CSS modules.`);
  console.error('Use CSS custom properties (var(--token)) instead of raw hex/rgb/rgba/hsl values.');
  console.error('If a raw color is intentional, add /* guard:allow — reason */ to the line.');
  process.exit(1);
} else {
  console.log('OK: no raw color literals detected in CSS modules.');
}
