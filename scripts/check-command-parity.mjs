#!/usr/bin/env node
/**
 * Command parity checker — validates that replacement IPC commands and
 * TypeScript API commands are kept in sync.
 *
 * Parses:
 * - core/src/ipc_v2.rs — normal command dispatch
 * - core/src/state_v2.rs — process-level commands that replace the active library
 * - src/platform/*.ts — invoke('command_name', ...) calls (per-domain API files)
 *
 * Reports any drift between the two surfaces, minus an explicit allowlist.
 */
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { extractRustCommandsFromText, extractTsCommandsFromText } from './check-command-parity-lib.mjs';

const ROOT = process.cwd();
const RUST_COMMAND_FILES = [
  path.join(ROOT, 'core/src/ipc_v2.rs'),
  path.join(ROOT, 'core/src/state_v2.rs'),
];
const CALLER_DIRS = [path.join(ROOT, 'src'), path.join(ROOT, 'electron')];
const ALLOWLIST_FILE = path.join(ROOT, 'scripts/command-parity-allowlist.json');

async function extractRustCommands() {
  const commands = new Set();
  const MOD_CMD_RE = /command\s*==\s*"([a-z_][a-z0-9_.]*)"/g;
  for (const file of RUST_COMMAND_FILES) {
    const content = await fs.readFile(file, 'utf8');
    for (const command of extractRustCommandsFromText(content)) commands.add(command);
    let match;
    while ((match = MOD_CMD_RE.exec(content)) !== null) {
      commands.add(match[1]);
    }
  }
  return commands;
}

async function extractTsCommands() {
  const commands = new Set();
  async function scan(dir) {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        await scan(fullPath);
      } else if (/\.(?:ts|tsx|mjs|js)$/.test(entry.name) && !/\.test\.[^.]+$/.test(entry.name)) {
        const content = await fs.readFile(fullPath, 'utf8');
        for (const cmd of extractTsCommandsFromText(content, fullPath)) commands.add(cmd);
      }
    }
  }
  for (const dir of CALLER_DIRS) await scan(dir);
  return commands;
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
