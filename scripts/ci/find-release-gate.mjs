#!/usr/bin/env node
// Resolve the one tested artifact set. Publication must never rebuild a tag.
import { execFileSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const packages = ['picto-macos-arm64', 'picto-windows-x64', 'picto-linux-x64'];
const reports = ['alpha-smoke-macos', 'alpha-smoke-windows', 'alpha-smoke-linux'];

export function findReleaseGate({ repository, sha, tag, version }, api) {
  if (!/^\d+\.\d+\.\d+-(?:alpha|rc)(?:[.-]\d+)?$/.test(version) || tag !== `v${version}`) {
    throw new Error('Release tag must match the alpha/rc package version.');
  }
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !/^[a-f0-9]{40}$/.test(sha)) {
    throw new Error('Missing repository or exact release commit.');
  }
  const base = `repos/${repository}`;
  const branch = `release/${version}`;
  const ref = api(`${base}/git/ref/heads/${branch}`);
  if (ref.object?.sha !== sha) throw new Error('Tag does not match the release branch.');
  const comparison = api(`${base}/compare/${sha}...main`);
  if (!['ahead', 'identical'].includes(comparison.status)) {
    throw new Error('Release commit must be landed on main before publication.');
  }
  const query = new URLSearchParams({ branch, head_sha: sha, event: 'push', status: 'success', per_page: '100' });
  const { workflow_runs: runs } = api(`${base}/actions/workflows/alpha-gate.yml/runs?${query}`);
  const run = runs.find(candidate => candidate.head_sha === sha && candidate.head_branch === branch
    && candidate.event === 'push' && candidate.status === 'completed' && candidate.conclusion === 'success'
    && candidate.path === '.github/workflows/alpha-gate.yml' && candidate.head_repository?.full_name === repository);
  if (!run) throw new Error('No successful release-branch gate for this exact commit. Do not retag; finish the gate first.');
  const { artifacts } = api(`${base}/actions/runs/${run.id}/artifacts?per_page=100`);
  const selected = [...packages, ...reports].map(name => {
    const matches = artifacts.filter(artifact => artifact.name === name && !artifact.expired && artifact.size_in_bytes > 0);
    if (matches.length !== 1) throw new Error(`Missing, expired, or ambiguous release artifact: ${name}. Rerun the release-branch gate.`);
    return matches[0];
  });
  return { runId: run.id, artifactIds: selected.slice(0, packages.length).map(artifact => artifact.id) };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
  const result = findReleaseGate({ repository: process.env.GITHUB_REPOSITORY, sha: process.env.GITHUB_SHA,
    tag: process.env.GITHUB_REF_NAME, version }, endpoint => JSON.parse(execFileSync('gh', ['api', endpoint], {
    encoding: 'utf8', timeout: 30_000,
  })));
  appendFileSync(process.env.GITHUB_OUTPUT, `run-id=${result.runId}\nartifact-ids=${result.artifactIds.join(',')}\n`);
  console.log('Verified release-branch gate, main ancestry, and all three package and smoke artifacts.');
}
