#!/usr/bin/env node
import { promises as fs } from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const RUST_EVENTS_FILE = path.join(ROOT, 'core/src/events.rs');
const TS_EVENTS_FILE = path.join(ROOT, 'src/types/api/events.ts');

function extractRustEventNames(content) {
  const moduleMatch = content.match(/pub mod event_names\s*\{([\s\S]*?)\n\}/m);
  if (!moduleMatch) {
    throw new Error('Could not find `event_names` module in core/src/events.rs');
  }
  const block = moduleMatch[1];
  const names = [];
  const re = /pub const [A-Z0-9_]+:\s*&str\s*=\s*"([^"]+)";/g;
  let match;
  while ((match = re.exec(block)) !== null) names.push(match[1]);
  return names;
}

function extractTsEventNames(content) {
  const mapMatch = content.match(/export interface CoreRuntimeEventPayloadMap\s*\{([\s\S]*?)\n\}/m);
  if (!mapMatch) {
    throw new Error('Could not find `CoreRuntimeEventPayloadMap` in src/types/api/events.ts');
  }
  const block = mapMatch[1];
  const names = [];
  const re = /['"]([^'"]+)['"]\s*:/g;
  let match;
  while ((match = re.exec(block)) !== null) names.push(match[1]);
  return names;
}

async function main() {
  const [rustText, tsText] = await Promise.all([
    fs.readFile(RUST_EVENTS_FILE, 'utf8'),
    fs.readFile(TS_EVENTS_FILE, 'utf8'),
  ]);

  const rustNames = extractRustEventNames(rustText);
  const tsNames = extractTsEventNames(tsText);

  const rustSet = new Set(rustNames);
  const tsSet = new Set(tsNames);

  const missingInTs = [...rustSet].filter((name) => !tsSet.has(name)).sort();
  const extraInTs = [...tsSet].filter((name) => !rustSet.has(name)).sort();

  const rustDupes = [...new Set(rustNames.filter((name, idx) => rustNames.indexOf(name) !== idx))].sort();
  const tsDupes = [...new Set(tsNames.filter((name, idx) => tsNames.indexOf(name) !== idx))].sort();

  let failed = false;

  console.log(`Rust runtime events: ${rustSet.size}`);
  console.log(`TS runtime events:   ${tsSet.size}`);

  if (missingInTs.length > 0) {
    failed = true;
    console.error('\nEvents declared in Rust but missing from CoreRuntimeEventPayloadMap:');
    for (const name of missingInTs) console.error(`  - ${name}`);
  }

  if (extraInTs.length > 0) {
    failed = true;
    console.error('\nEvents declared in CoreRuntimeEventPayloadMap but missing from Rust event_names:');
    for (const name of extraInTs) console.error(`  - ${name}`);
  }

  if (rustDupes.length > 0) {
    failed = true;
    console.error('\nDuplicate Rust event names detected:');
    for (const name of rustDupes) console.error(`  - ${name}`);
  }

  if (tsDupes.length > 0) {
    failed = true;
    console.error('\nDuplicate TS event names detected in CoreRuntimeEventPayloadMap:');
    for (const name of tsDupes) console.error(`  - ${name}`);
  }

  if (failed) {
    console.error('\nRuntime event schema check FAILED.');
    process.exit(1);
  }

  console.log('Runtime event schema check passed.');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
