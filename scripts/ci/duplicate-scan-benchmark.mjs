#!/usr/bin/env node

import fs from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const require = createRequire(import.meta.url);
const native = require('../../native/picto-node/index.node');

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index];
    if (key === '--help') args.help = true;
    else if (key.startsWith('--')) {
      args[key.slice(2)] = argv[index + 1] ?? true;
      index += 1;
    }
  }
  return args;
}

function usage() {
  console.log(`Usage: node scripts/ci/duplicate-scan-benchmark.mjs --library <path> [options]

Measures the real native duplicate scanner against the supplied library. The library is not
generated and no scale claim is made; the JSON report records the observed population only.

Options:
  --library <path>      existing Picto library (required)
  --report <path>       JSON report path (default: artifacts/duplicates/scan-benchmark.json)
  --repeat <count>      number of scans (default: 1)
  --threshold <value>   perceptual distance threshold passed to the scanner
`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) return usage();
  if (!args.library) throw new Error('--library is required');
  const library = path.resolve(args.library);
  const reportPath = path.resolve(args.report || 'artifacts/duplicates/scan-benchmark.json');
  const repeat = Math.max(1, Number(args.repeat || 1));
  const threshold = args.threshold === undefined ? null : Number(args.threshold);
  const runs = [];
  const startedAt = new Date().toISOString();

  native.initRuntime();
  await native.openLibrary(library);
  try {
    for (let index = 0; index < repeat; index += 1) {
      const started = performance.now();
      const summary = JSON.parse(await native.invoke('scan_duplicates', JSON.stringify({ threshold })));
      const elapsedMs = performance.now() - started;
      runs.push({
        iteration: index + 1,
        elapsed_ms: Number(elapsedMs.toFixed(3)),
        total_files: summary.total_files,
        files_scanned: summary.files_scanned,
        files_with_phash: summary.files_with_phash,
        candidates_found: summary.candidates_found,
        pairs_inserted: summary.pairs_inserted,
        reviewable_detected_total: summary.reviewable_detected_total,
      });
    }
  } finally {
    await native.closeLibrary();
  }

  const elapsed = runs.map((run) => run.elapsed_ms);
  const report = {
    kind: 'duplicate-scan-benchmark',
    library_root: library,
    started_at: startedAt,
    finished_at: new Date().toISOString(),
    threshold,
    repeat,
    observed: {
      total_files: runs[0]?.total_files ?? 0,
      files_with_phash: runs[0]?.files_with_phash ?? 0,
      candidates_found: runs[0]?.candidates_found ?? 0,
    },
    runs,
    best_elapsed_ms: Math.min(...elapsed),
    mean_elapsed_ms: Number((elapsed.reduce((sum, value) => sum + value, 0) / elapsed.length).toFixed(3)),
    scale_claim: 'none; this report describes only the supplied library at measurement time',
  };
  await fs.mkdir(path.dirname(reportPath), { recursive: true });
  await fs.writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`[duplicate-benchmark] wrote ${reportPath}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  void main().catch((error) => {
    console.error(`[duplicate-benchmark] ${error.message}`);
    process.exitCode = 1;
  });
}

export { parseArgs };
