import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import yaml from 'js-yaml';
import { findReleaseGate } from './find-release-gate.mjs';

const input = { repository: 'example/picto', sha: 'a'.repeat(40), tag: 'v0.6.13-alpha', version: '0.6.13-alpha' };
function fixture() {
  const run = { id: 42, head_sha: input.sha, head_branch: 'release/0.6.13-alpha', event: 'push',
    status: 'completed', conclusion: 'success', path: '.github/workflows/alpha-gate.yml',
    head_repository: { full_name: input.repository } };
  const state = { ref: input.sha, comparison: 'identical', runs: [run], artifacts: [
    'picto-macos-arm64', 'picto-windows-x64', 'picto-linux-x64',
    'alpha-smoke-macos', 'alpha-smoke-windows', 'alpha-smoke-linux',
  ].map((name, index) => ({ id: index + 1, name, expired: false, size_in_bytes: 100 })) };
  const api = endpoint => {
    if (endpoint.includes('/git/ref/')) return { object: { sha: state.ref } };
    if (endpoint.includes('/compare/')) return { status: state.comparison };
    if (endpoint.includes('/workflows/')) return { workflow_runs: state.runs };
    if (endpoint.includes('/runs/42/artifacts')) return { artifacts: state.artifacts };
    throw new Error(`Unexpected endpoint: ${endpoint}`);
  };
  return { state, api };
}

describe('release artifact provenance', () => {
  it.each(['identical', 'ahead'])('selects only three package IDs when main is %s', comparison => {
    const { state, api } = fixture();
    state.comparison = comparison;
    expect(findReleaseGate(input, api)).toEqual({ runId: 42, artifactIds: [1, 2, 3] });
  });
  it('rejects tag/version disagreement before reading GitHub', () => {
    expect(() => findReleaseGate({ ...input, tag: 'v0.6.12-alpha' }, () => { throw new Error('API called'); }))
      .toThrow('tag must match');
  });
  it('rejects a release branch that moved away from the tagged commit', () => {
    const { state, api } = fixture(); state.ref = 'b'.repeat(40);
    expect(() => findReleaseGate(input, api)).toThrow('does not match the release branch');
  });
  it.each(['behind', 'diverged'])('rejects a commit absent from main (%s)', comparison => {
    const { state, api } = fixture(); state.comparison = comparison;
    expect(() => findReleaseGate(input, api)).toThrow('landed on main');
  });
  it.each([
    { head_sha: 'b'.repeat(40) }, { head_branch: 'main' }, { event: 'pull_request' },
    { status: 'in_progress' }, { conclusion: 'failure' }, { conclusion: 'cancelled' },
    { path: '.github/workflows/other.yml' }, { head_repository: { full_name: 'fork/picto' } },
  ])('rejects unrelated, unfinished, or failed evidence: %j', changed => {
    const { state, api } = fixture(); Object.assign(state.runs[0], changed);
    expect(() => findReleaseGate(input, api)).toThrow('No successful release-branch gate');
  });
  it.each([0, 1, 2, 3, 4, 5])('requires each platform package and smoke report (%i)', index => {
    const { state, api } = fixture(); state.artifacts.splice(index, 1);
    expect(() => findReleaseGate(input, api)).toThrow('Missing, expired, or ambiguous');
  });
  it.each([{ expired: true }, { size_in_bytes: 0 }])('rejects unusable artifacts: %j', changed => {
    const { state, api } = fixture(); Object.assign(state.artifacts[0], changed);
    expect(() => findReleaseGate(input, api)).toThrow('Missing, expired, or ambiguous');
  });
});

it('runs packaging beside verification, then publishes existing artifacts without rebuilding', () => {
  const workflow = yaml.load(readFileSync('.github/workflows/alpha-gate.yml', 'utf8'));
  const jobs = workflow.jobs;
  expect(jobs['alpha-package'].needs).toBeUndefined();
  expect(jobs['alpha-package'].if).toContain("!startsWith(github.ref, 'refs/tags/')");
  expect(jobs['alpha-package'].if).toContain("github.event_name != 'pull_request'");
  expect(jobs['alpha-package'].strategy.matrix.include.map(row => row.platform).sort()).toEqual(['linux', 'macos', 'windows']);
  const publish = jobs['alpha-release-assets'];
  expect(publish.needs).toBeUndefined();
  expect(publish.permissions).toEqual({ contents: 'write', actions: 'read' });
  const download = publish.steps.find(step => step.uses?.startsWith('actions/download-artifact@'));
  expect(download.with['run-id']).toBe('${{ steps.gate.outputs.run-id }}');
  expect(download.with['artifact-ids']).toBe('${{ steps.gate.outputs.artifact-ids }}');
  expect(download.with['digest-mismatch']).toBe('error');
  expect(publish.steps.some(step => step.run?.includes('alpha:package'))).toBe(false);
});
