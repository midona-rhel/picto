#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const token = argv[i];
    if (token === '--platform') out.platform = argv[++i];
    else if (token === '--report') out.report = argv[++i];
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const platform = args.platform || process.platform;
const startedAt = new Date().toISOString();
const reportPath = args.report || path.join('artifacts', 'alpha-smoke', `${platform}.json`);

const scenarios = [
  {
    id: 'launch_app',
    label: 'Launch binary sanity',
    command: 'npx electron --version',
    env: {
      // Avoid GUI/process bootstrap differences on local runners while still
      // asserting the bundled Electron runtime is callable.
      ELECTRON_RUN_AS_NODE: '1',
    },
  },
  {
    id: 'open_library_and_import_path',
    label: 'Core orchestration smoke',
    command: 'cargo test --manifest-path core/Cargo.toml --quiet --test orchestration',
  },
  {
    id: 'inbox_render_and_subscription_updates',
    label: 'Inbox live update smoke',
    command: 'npm run test -- src/stores/__tests__/eventBridge.inboxSubscriptionImport.test.ts --run',
  },
  {
    id: 'status_move_trash_inbox',
    label: 'Sidebar status move smoke',
    command: 'npm run test -- src/components/sidebar/__tests__/Sidebar.drag-drop.test.tsx --run',
  },
];

const results = [];
let allPassed = true;

for (const scenario of scenarios) {
  const t0 = Date.now();
  const env = {
    ...process.env,
    ...(scenario.env || {}),
  };
  const proc = spawnSync(scenario.command, {
    shell: true,
    encoding: 'utf8',
    env,
    maxBuffer: 1024 * 1024 * 20,
  });
  const durationMs = Date.now() - t0;
  const passed = proc.status === 0;
  allPassed = allPassed && passed;
  results.push({
    id: scenario.id,
    label: scenario.label,
    command: scenario.command,
    passed,
    exit_code: proc.status,
    duration_ms: durationMs,
    stdout_tail: (proc.stdout || '').split('\n').slice(-40).join('\n'),
    stderr_tail: (proc.stderr || '').split('\n').slice(-40).join('\n'),
  });
  const status = passed ? 'PASS' : 'FAIL';
  console.log(`[alpha-smoke] ${status} ${scenario.id} (${durationMs}ms)`);
}

const report = {
  platform,
  started_at: startedAt,
  finished_at: new Date().toISOString(),
  all_passed: allPassed,
  scenarios: results,
};

fs.mkdirSync(path.dirname(reportPath), { recursive: true });
fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
console.log(`[alpha-smoke] wrote report to ${reportPath}`);

if (!allPassed) {
  process.exit(1);
}
