#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);

function value(flag) {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

if (args.includes('--help')) {
  console.log(`Usage:
  npm run subscriptions:verify-sites -- --site danbooru --query "artist_name"
  npm run subscriptions:verify-sites -- --site SOURCE_ID --query "known query" [--post-limit 100] [--report PATH] [--credential-file PATH]

The default certification uses a fresh library and requires exactly 100 posts
in the first fetch. It proves every advertised media item, post identity, source
order, metadata, blob, close/reopen, resume, and idempotent replay, then writes a
JSON evidence report.
Use --credential-file for unattended certification without touching the OS
keychain. Use --allow-keychain only for an explicitly attended local run.
Source support and credential requirements are owned by the backend registry.`);
  process.exit(0);
}

const site = value('--site');
const query = value('--query');
const postLimitRaw = value('--post-limit') ?? '100';
const postLimit = Number.parseInt(postLimitRaw, 10);

if (!site || !query) {
  console.error('Both --site and --query are required. Use --help for examples.');
  process.exit(2);
}
if (!Number.isSafeInteger(postLimit) || postLimit < 1) {
  console.error('--post-limit must be a positive integer.');
  process.exit(2);
}

const timestamp = new Date().toISOString().replaceAll(/[:.]/g, '-');
const safeSite = site.replaceAll(/[^a-zA-Z0-9_-]/g, '_');
const report = path.resolve(
  root,
  value('--report') ?? `artifacts/subscription-certification/${safeSite}-${timestamp}.json`,
);

const env = {
  ...process.env,
  PICTO_LIVE_SUBSCRIPTION_SITE: site,
  PICTO_LIVE_SUBSCRIPTION_QUERY: query,
  PICTO_LIVE_SUBSCRIPTION_POST_LIMIT: String(postLimit),
  PICTO_LIVE_SUBSCRIPTION_REPORT: report,
  PICTO_LIVE_SUBSCRIPTION_TIMEOUT_SECONDS:
    process.env.PICTO_LIVE_SUBSCRIPTION_TIMEOUT_SECONDS ?? '7200',
};
const credentialFile = value('--credential-file');
if (credentialFile) {
  env.PICTO_LIVE_SUBSCRIPTION_CREDENTIAL_FILE = path.resolve(credentialFile);
}
if (args.includes('--allow-keychain')) {
  env.PICTO_LIVE_SUBSCRIPTION_ALLOW_KEYCHAIN = '1';
}

const result = spawnSync(
  'cargo',
  [
    'test',
    '--manifest-path',
    'core/Cargo.toml',
    '--test',
    'subscription_source_readiness',
    'live_subscription_source_persistence_certification',
    '--',
    '--ignored',
    '--exact',
    '--nocapture',
  ],
  { cwd: root, env, stdio: 'inherit' },
);

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
