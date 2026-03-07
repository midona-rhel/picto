#!/usr/bin/env node
/**
 * Command parity checker — validates that Rust dispatch commands and
 * TypeScript API commands are kept in sync.
 *
 * Parses:
 * - core/src/dispatch/*.rs — match arm string literals ("command_name" =>)
 * - src/desktop/api.ts — invoke('command_name', ...) calls
 *
 * Reports any drift between the two surfaces, minus an explicit allowlist.
 */
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { extractRustCommandsFromText, extractTsCommandsFromText } from './check-command-parity-lib.mjs';

const ROOT = process.cwd();
const DISPATCH_DIR = path.join(ROOT, 'core/src/dispatch');
const API_FILE = path.join(ROOT, 'src/desktop/api.ts');
const ALLOWLIST_FILE = path.join(ROOT, 'scripts/command-parity-allowlist.json');

async function extractRustCommands() {
  const commands = new Set();

  // Recursively collect .rs files from dispatch directory (including typed/ subdirs)
  async function scanDir(dir) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        await scanDir(fullPath);
      } else if (entry.name.endsWith('.rs') && entry.name !== 'common.rs') {
        const content = await fs.readFile(fullPath, 'utf8');
        for (const cmd of extractRustCommandsFromText(content)) commands.add(cmd);
        // Also match TypedCommand const NAME patterns: const NAME: &'static str = "cmd";
        const TYPED_CMD_RE = /const\s+NAME:\s*&'static\s+str\s*=\s*"([a-z_]+)"/g;
        let m;
        while ((m = TYPED_CMD_RE.exec(content)) !== null) {
          commands.add(m[1]);
        }
      }
    }
  }
  await scanDir(DISPATCH_DIR);

  // Also catch inline commands in mod.rs dispatch fn (e.g., close_library)
  const modContent = await fs.readFile(path.join(DISPATCH_DIR, 'mod.rs'), 'utf8');
  const MOD_CMD_RE = /command\s*==\s*"([a-z_]+)"/g;
  let match;
  while ((match = MOD_CMD_RE.exec(modContent)) !== null) {
    commands.add(match[1]);
  }
  return commands;
}

async function extractTsCommands() {
  const content = await fs.readFile(API_FILE, 'utf8');
  return extractTsCommandsFromText(content, API_FILE);
}

async function loadAllowlist() {
  try {
    const content = await fs.readFile(ALLOWLIST_FILE, 'utf8');
    const data = JSON.parse(content);
    return {
      rustOnly: new Set(data.rust_only || []),
      tsOnly: new Set(data.ts_only || []),
    };
  } catch {
    return { rustOnly: new Set(), tsOnly: new Set() };
  }
}

async function main() {
  const [rustCmds, tsCmds, allowlist] = await Promise.all([
    extractRustCommands(),
    extractTsCommands(),
    loadAllowlist(),
  ]);

  console.log(`Rust dispatch commands: ${rustCmds.size}`);
  console.log(`TypeScript API commands: ${tsCmds.size}`);
  console.log(`Allowlist rust-only: ${allowlist.rustOnly.size}, ts-only: ${allowlist.tsOnly.size}`);

  // Find drift
  const rustOnly = [...rustCmds].filter((c) => !tsCmds.has(c) && !allowlist.rustOnly.has(c));
  const tsOnly = [...tsCmds].filter((c) => !rustCmds.has(c) && !allowlist.tsOnly.has(c));

  // Check for stale allowlist entries
  const staleRustOnly = [...allowlist.rustOnly].filter((c) => !rustCmds.has(c));
  const staleTsOnly = [...allowlist.tsOnly].filter((c) => !tsCmds.has(c));

  let hasErrors = false;

  if (rustOnly.length > 0) {
    console.error(`\nRust-only commands (not in TS API and not allowlisted):`);
    for (const c of rustOnly.sort()) console.error(`  - ${c}`);
    hasErrors = true;
  }

  if (tsOnly.length > 0) {
    console.error(`\nTS-only commands (not in Rust dispatch and not allowlisted):`);
    for (const c of tsOnly.sort()) console.error(`  - ${c}`);
    hasErrors = true;
  }

  if (staleRustOnly.length > 0) {
    console.warn(`\nStale rust_only allowlist entries (command no longer in Rust):`);
    for (const c of staleRustOnly.sort()) console.warn(`  - ${c}`);
  }

  if (staleTsOnly.length > 0) {
    console.warn(`\nStale ts_only allowlist entries (command no longer in TS):`);
    for (const c of staleTsOnly.sort()) console.warn(`  - ${c}`);
  }

  if (hasErrors) {
    console.error('\nCommand parity check FAILED.');
    console.error('To fix: add missing commands to the other side, or add to scripts/command-parity-allowlist.json.');
    process.exit(1);
  }

  console.log('\nCommand parity check passed.');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
