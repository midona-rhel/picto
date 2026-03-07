#!/usr/bin/env node
/**
 * Typed command parity checker (PBI-234).
 *
 * Validates that:
 * 1. Every TypedCommand::NAME in Rust has a matching entry in TypedCommandMap in TS.
 * 2. No typed command still exists in a legacy handler's match block (no duplicates).
 * 3. Generated ts-rs files exist for every input/output struct.
 * 4. No typed command uses plain invoke() in api.ts (should use invokeTyped).
 * 5. Runtime contract generated files exist (PBI-325).
 */
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const TYPED_DIR = path.join(ROOT, 'core/src/dispatch/typed');
const LEGACY_DIR = path.join(ROOT, 'core/src/dispatch');
const TS_BARREL = path.join(ROOT, 'src/types/generated/commands/index.ts');
const GENERATED_DIR = path.join(ROOT, 'src/types/generated/commands');
const API_TS = path.join(ROOT, 'src/desktop/api.ts');
const RUNTIME_CONTRACT_DIR = path.join(ROOT, 'src/types/generated/runtime-contract');

// Extract TypedCommand NAME constants from Rust typed dispatch files.
const TYPED_CMD_RE = /const\s+NAME:\s*&'static\s+str\s*=\s*"([a-z_]+)"/g;

// Match legacy match arm command strings: "command_name" =>
const LEGACY_CMD_LINE_RE = /^\s*"([a-z_]+)"\s*(?:\||\s*=>)/;

async function extractTypedCommands() {
  const commands = new Set();
  const entries = await fs.readdir(TYPED_DIR);
  for (const entry of entries) {
    if (!entry.endsWith('.rs') || entry === 'mod.rs') continue;
    const content = await fs.readFile(path.join(TYPED_DIR, entry), 'utf8');
    let m;
    TYPED_CMD_RE.lastIndex = 0;
    while ((m = TYPED_CMD_RE.exec(content)) !== null) {
      commands.add(m[1]);
    }
  }
  return commands;
}

async function extractLegacyCommands() {
  const commands = new Set();
  const entries = await fs.readdir(LEGACY_DIR);
  for (const entry of entries) {
    if (!entry.endsWith('.rs') || entry === 'common.rs' || entry === 'mod.rs') continue;
    const content = await fs.readFile(path.join(LEGACY_DIR, entry), 'utf8');
    for (const line of content.split('\n')) {
      const m = LEGACY_CMD_LINE_RE.exec(line);
      if (m) commands.add(m[1]);
    }
  }
  return commands;
}

async function extractTsTypedCommandMap() {
  const content = await fs.readFile(TS_BARREL, 'utf8');
  const commands = new Set();
  // Find TypedCommandMap interface, then extract entry keys line by line.
  // Handle nested braces by tracking brace depth.
  const lines = content.split('\n');
  let inside = false;
  let depth = 0;
  for (const line of lines) {
    if (!inside && /interface\s+TypedCommandMap/.test(line)) {
      inside = true;
      depth = 0;
    }
    if (inside) {
      depth += (line.match(/\{/g) || []).length;
      depth -= (line.match(/\}/g) || []).length;
      // Match top-level entries (depth 1 = inside the interface, not inside a nested {})
      const entryMatch = line.match(/^\s*([a-z_]+)\s*:/);
      if (entryMatch && depth >= 1) {
        commands.add(entryMatch[1]);
      }
      if (depth <= 0 && inside) {
        inside = false;
      }
    }
  }
  return commands;
}

// Scan api.ts for plain invoke() calls that use a typed command name.
// These should use invokeTyped() instead.
const PLAIN_INVOKE_RE = /\binvoke\s*<[^>]*>\s*\(\s*'([a-z_]+)'/g;

async function extractPlainInvokeCommands() {
  const content = await fs.readFile(API_TS, 'utf8');
  const commands = new Map(); // command → line number
  const lines = content.split('\n');
  for (let i = 0; i < lines.length; i++) {
    PLAIN_INVOKE_RE.lastIndex = 0;
    let m;
    while ((m = PLAIN_INVOKE_RE.exec(lines[i])) !== null) {
      commands.set(m[1], i + 1);
    }
  }
  return commands;
}

async function checkGeneratedFilesExist() {
  const missing = [];
  const entries = await fs.readdir(GENERATED_DIR);
  const tsFiles = new Set(entries.filter((e) => e.endsWith('.ts') && e !== 'index.ts'));
  // Basic check: at least some .ts files exist (ts-rs generates them)
  if (tsFiles.size === 0) {
    missing.push('No generated .ts files found. Run `cargo test --lib export_bindings`.');
  }
  return missing;
}

// Runtime contract types that must be generated from Rust (PBI-325).
const RUNTIME_CONTRACT_FILES = [
  'DerivedInvalidation.ts',
  'Domain.ts',
  'MutationFacts.ts',
  'MutationReceipt.ts',
  'RuntimeSnapshot.ts',
  'RuntimeTask.ts',
  'SidebarCounts.ts',
  'TaskKind.ts',
  'TaskProgress.ts',
  'TaskStatus.ts',
];

async function checkRuntimeContractFiles() {
  const missing = [];
  let entries;
  try {
    entries = await fs.readdir(RUNTIME_CONTRACT_DIR);
  } catch {
    missing.push('Runtime contract directory missing: src/types/generated/runtime-contract/');
    return missing;
  }
  const existing = new Set(entries);
  for (const file of RUNTIME_CONTRACT_FILES) {
    if (!existing.has(file)) {
      missing.push(file);
    }
  }
  return missing;
}

async function main() {
  const [typedCmds, legacyCmds, tsMapCmds, fileMissing, plainInvokeCmds, rtContractMissing] = await Promise.all([
    extractTypedCommands(),
    extractLegacyCommands(),
    extractTsTypedCommandMap(),
    checkGeneratedFilesExist(),
    extractPlainInvokeCommands(),
    checkRuntimeContractFiles(),
  ]);

  console.log(`Typed Rust commands: ${typedCmds.size}`);
  console.log(`TypedCommandMap TS entries: ${tsMapCmds.size}`);

  let hasErrors = false;

  // 1. Every typed command must be in TypedCommandMap
  const missingInTs = [...typedCmds].filter((c) => !tsMapCmds.has(c));
  if (missingInTs.length > 0) {
    console.error('\nTyped Rust commands missing from TypedCommandMap:');
    for (const c of missingInTs.sort()) console.error(`  - ${c}`);
    hasErrors = true;
  }

  // 2. Every TypedCommandMap entry must have a Rust TypedCommand
  const extraInTs = [...tsMapCmds].filter((c) => !typedCmds.has(c));
  if (extraInTs.length > 0) {
    console.error('\nTypedCommandMap entries with no Rust TypedCommand:');
    for (const c of extraInTs.sort()) console.error(`  - ${c}`);
    hasErrors = true;
  }

  // 3. No typed command should also be in a legacy handler
  const duplicated = [...typedCmds].filter((c) => legacyCmds.has(c));
  if (duplicated.length > 0) {
    console.error('\nCommands duplicated in both typed and legacy dispatch:');
    for (const c of duplicated.sort()) console.error(`  - ${c}`);
    hasErrors = true;
  }

  // 4. Generated files must exist
  if (fileMissing.length > 0) {
    for (const msg of fileMissing) console.error(`\n${msg}`);
    hasErrors = true;
  }

  // 5. No typed command should use plain invoke() in api.ts (PBI-324)
  const untypedFrontend = [...plainInvokeCmds.entries()]
    .filter(([cmd]) => tsMapCmds.has(cmd));
  if (untypedFrontend.length > 0) {
    console.error('\nTyped commands using plain invoke() in api.ts (should use invokeTyped):');
    for (const [cmd, line] of untypedFrontend.sort((a, b) => a[0].localeCompare(b[0]))) {
      console.error(`  - ${cmd} (line ${line})`);
    }
    hasErrors = true;
  }

  // 6. Runtime contract generated files must exist (PBI-325)
  if (rtContractMissing.length > 0) {
    console.error('\nMissing runtime contract generated files (run `cargo test --lib`):');
    for (const f of rtContractMissing) console.error(`  - ${f}`);
    hasErrors = true;
  }

  if (hasErrors) {
    console.error('\nTyped command check FAILED.');
    process.exit(1);
  }

  console.log('\nTyped command check passed.');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
