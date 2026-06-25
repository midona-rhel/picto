#!/usr/bin/env node
/**
 * Per-site subscription verification CLI.
 *
 * Probes each configured site end to end (URL build → gallery-dl run →
 * metadata parse → schema validation) against a scratch library, without
 * ingesting anything. Live network — manual dev lane, never CI.
 *
 * Usage:
 *   node scripts/verify-sites.mjs                 # all sites
 *   node scripts/verify-sites.mjs --site danbooru # one site
 *   node scripts/verify-sites.mjs --site danbooru --query "1girl"
 *   node scripts/verify-sites.mjs --strict-auth   # credential-missing counts as failure
 *
 * Stored credentials (OS keychain) are used automatically when present.
 * Writes a JSON report to artifacts/site-verification/report.json.
 */
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);

const args = process.argv.slice(2);
function argValue(flag) {
  const idx = args.indexOf(flag);
  return idx >= 0 && idx + 1 < args.length ? args[idx + 1] : null;
}
const onlySite = argValue('--site');
const queryOverride = argValue('--query');
const strictAuth = args.includes('--strict-auth');

let binding;
try {
  binding = require(path.join(ROOT, 'native/picto-node/index.node'));
} catch (error) {
  console.error(`Failed to load native addon: ${error}`);
  console.error('Build it first: npm run rebuild:native');
  process.exit(1);
}

async function invoke(command, cmdArgs = {}) {
  const resultJson = await binding.invoke(command, JSON.stringify(cmdArgs));
  if (resultJson == null || resultJson === 'null' || resultJson === '') return null;
  return JSON.parse(resultJson);
}

function fmtRow(cols, widths) {
  return cols.map((c, i) => String(c ?? '').padEnd(widths[i]).slice(0, widths[i])).join('  ');
}

async function main() {
  // Scratch library — verification never ingests, but the addon needs an open library.
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-verify-'));
  binding.initRuntime?.();
  await binding.initialize(scratch);
  await binding.openLibrary(scratch);

  const sites = await invoke('get_sites');
  const targets = onlySite ? sites.filter((s) => s.id === onlySite) : sites;
  if (targets.length === 0) {
    console.error(onlySite ? `Unknown site: ${onlySite}` : 'No sites configured.');
    process.exit(1);
  }

  const reports = [];
  const widths = [16, 6, 12, 10, 6, 22, 40];
  console.log(fmtRow(['SITE', 'PASS', 'CREDENTIAL', 'DISC/DOWN', 'TAGS', 'FAILURE KIND', 'REASONS'], widths));
  console.log('-'.repeat(widths.reduce((a, b) => a + b + 2, 0)));

  for (const site of targets) {
    let report;
    try {
      report = await invoke('verify_subscription_site', {
        site_id: site.id,
        query: queryOverride,
        post_limit: 2,
      });
    } catch (error) {
      report = {
        site_id: site.id,
        url: '',
        credential_state: '?',
        exit_code: null,
        failure_kind: 'invoke_error',
        stderr_tail: String(error),
        discovered: 0,
        downloaded: 0,
        skipped_archive: 0,
        items: [],
        passed: false,
        failure_reasons: [`invoke failed: ${error}`],
      };
    }
    reports.push(report);

    const skippedAuth = report.failure_reasons?.[0] === 'skipped: credential missing'
      || report.failure_reasons?.[0]?.startsWith('inconclusive:');
    const totalTags = (report.items ?? []).reduce((a, i) => a + (i.tag_count ?? 0), 0);
    console.log(fmtRow([
      report.site_id,
      report.passed ? 'ok' : skippedAuth ? 'skip' : 'FAIL',
      report.credential_state,
      `${report.discovered}/${report.downloaded}`,
      totalTags,
      report.failure_kind ?? '',
      (report.failure_reasons ?? []).join('; '),
    ], widths));
  }

  const outDir = path.join(ROOT, 'artifacts', 'site-verification');
  await fs.mkdir(outDir, { recursive: true });
  const outPath = path.join(outDir, 'report.json');
  await fs.writeFile(outPath, JSON.stringify(reports, null, 2));
  console.log(`\nFull report: ${outPath}`);

  const failures = reports.filter((r) => {
    if (r.passed) return false;
    const authSkip = r.failure_reasons?.[0] === 'skipped: credential missing';
    if (authSkip && !strictAuth) return false;
    // Placeholder account queries prove the extractor runs, nothing more.
    if (r.failure_reasons?.[0]?.startsWith('inconclusive:')) return false;
    // Rate limits and network blips are inconclusive, not failures.
    if (r.failure_kind === 'rate_limited' || r.failure_kind === 'network') return false;
    return true;
  });

  console.log(`\n${reports.length} probed, ${reports.filter((r) => r.passed).length} passed, ${failures.length} actionable failures.`);
  await binding.closeLibrary?.();
  process.exit(failures.length > 0 ? 1 : 0);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
