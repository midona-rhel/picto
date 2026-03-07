/**
 * Guard: scope business-rule comments in consumer files must cite the canonical contract.
 *
 * Scans Rust files under core/src/ (excluding core/src/scope/) for comments
 * that describe scope business rules. If a file contains such comments
 * without referencing the canonical source (scope::resolver or scope/resolver),
 * the guard fails.
 *
 * This enforces PBI-301 acceptance criterion 3:
 *   "Comments cannot contradict the tested contract without failing review."
 *
 * The canonical scope definitions live in:
 *   - core/src/scope/resolver.rs (resolve_scope, scope_count)
 *   - core/tests/orchestration.rs (scope_contract_* conformance tests)
 *
 * Exemptions:
 *   - Lines containing `guard:allow` in a comment are skipped.
 *   - Files inside core/src/scope/ (the canonical source itself).
 *   - Test files and test modules (lines after `#[cfg(test)]`).
 */

import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const coreDir = path.join(root, 'core', 'src');
const scopeDir = path.join(coreDir, 'scope');

// Patterns that indicate a comment is describing scope business rules.
// These are terms that, when used in a comment, suggest the author is
// restating scope semantics that should live in scope::resolver.
const SCOPE_RULE_PATTERNS = [
  // Scope identity definitions: "system:all means ...", "system:inbox includes ..."
  /\bsystem:(all|inbox|trash|untagged|uncategorized)\b.*\b(means|equals|includes|excludes)\b/i,
  // Scope membership definitions: "untagged means active items ..."
  /\b(untagged|uncategorized)\s+(means|equals|is defined as)\b/i,
  // Scope resolution logic descriptions
  /\bscope\s+(resolution|semantic|rule|logic|definition)\b/i,
  // Select-all semantics
  /\bselect.all\b.*\b(means|equals|scope|visible|membership)\b/i,
];

// A file that contains scope-rule comments must also reference the canonical source.
const CANONICAL_REFS = [
  /scope::resolver/,
  /scope\/resolver/,
  /scope_contract/,
  /crate::scope/,
];

// Skip lines with this exemption marker.
const GUARD_ALLOW = /guard:allow/;

function isCommentLine(line) {
  const trimmed = line.trim();
  return trimmed.startsWith('//') || trimmed.startsWith('*') || trimmed.startsWith('///');
}

function scanFile(filePath) {
  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split('\n');
  const violations = [];
  let inTestModule = false;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Skip test modules entirely
    if (/^\s*#\[cfg\(test\)\]/.test(line)) {
      inTestModule = true;
    }
    if (inTestModule) continue;

    // Only check comment lines
    if (!isCommentLine(line)) continue;

    // Skip exempted lines
    if (GUARD_ALLOW.test(line)) continue;

    for (const pattern of SCOPE_RULE_PATTERNS) {
      if (pattern.test(line)) {
        violations.push({ line: i + 1, text: line.trim() });
        break;
      }
    }
  }

  if (violations.length === 0) return [];

  // Check if the file references the canonical source anywhere
  for (const ref of CANONICAL_REFS) {
    if (ref.test(content)) return []; // File cites the canonical source — OK
  }

  return violations;
}

function findRustFiles(dir) {
  const results = [];
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return results;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      // Skip the canonical scope module itself
      if (full === scopeDir) continue;
      if (entry.name === 'target') continue;
      results.push(...findRustFiles(full));
    } else if (entry.name.endsWith('.rs')) {
      results.push(full);
    }
  }
  return results;
}

const files = findRustFiles(coreDir);
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
  console.error(
    `\nFAILED: ${totalViolations} scope business-rule comment(s) without canonical contract reference.`
  );
  console.error(
    'Consumer files describing scope semantics must reference scope::resolver or scope/resolver.'
  );
  console.error(
    'Either add a reference (e.g. "// See scope::resolver for canonical rules") or add /* guard:allow — reason */ to the line.'
  );
  process.exit(1);
} else {
  console.log('OK: scope business-rule comments cite the canonical contract.');
}
