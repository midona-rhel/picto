import { describe, expect, it } from 'vitest';
import { checkBranchPolicy } from './check-branch-policy.mjs';

const repository = 'example/picto';
const sha = 'a'.repeat(40);
function pr(base, head, number = 100) {
  return { number, base: { ref: base }, head: { ref: head, sha, repo: { full_name: repository } } };
}
const noApi = () => { throw new Error('Unexpected API request'); };
function gate({ completed = true, missingArtifact = false } = {}) {
  return endpoint => {
    if (endpoint.includes('/contents/')) return { content: Buffer.from(JSON.stringify({ version: '1.0.0-alpha' })).toString('base64') };
    if (endpoint.includes('/git/ref/')) return { object: { sha } };
    if (endpoint.includes('/workflows/')) return { workflow_runs: completed ? [{ id: 42, head_sha: sha,
      head_branch: 'release/1.0.0-alpha', event: 'push', status: 'completed', conclusion: 'success',
      path: '.github/workflows/alpha-gate.yml', head_repository: { full_name: repository } }] : [] };
    if (endpoint.includes('/artifacts')) return { artifacts: (missingArtifact ? [] : [
      'picto-macos-arm64', 'picto-windows-x64', 'picto-linux-x64',
      'alpha-smoke-macos', 'alpha-smoke-windows', 'alpha-smoke-linux',
    ]).map((name, id) => ({ id, name, expired: false, size_in_bytes: 100 })) };
    throw new Error(`Unexpected API request: ${endpoint}`);
  };
}
describe('protected branch policy', () => {
  it.each(['bug/123-fix', 'main', 'dependabot/npm/x'])('accepts development branch %s', head => {
    expect(() => checkBranchPolicy(pr('dev', head), repository, noApi)).not.toThrow();
  });
  it.each(['feature/no-ticket', 'random', 'release/1.0.0-alpha'])('rejects an untracked work branch %s', head => {
    expect(() => checkBranchPolicy(pr('dev', head), repository, noApi)).toThrow('issue number');
  });
  it.each(['main', 'dependabot/npm/x'])('rejects a fork borrowing the maintenance name %s', head => {
    const pull = pr('dev', head);
    pull.head.repo.full_name = 'fork/picto';
    expect(() => checkBranchPolicy(pull, repository, noApi)).toThrow('issue number');
  });
  it('accepts a numbered work branch from a fork', () => {
    const pull = pr('dev', 'bug/123-fix');
    pull.head.repo.full_name = 'fork/picto';
    expect(() => checkBranchPolicy(pull, repository, noApi)).not.toThrow();
  });
  it('permits the existing workflow PR during bootstrap but rejects it after retargeting to main', () => {
    const pull = pr('dev', 'chore/contribution-workflow', 55);
    expect(() => checkBranchPolicy(pull, repository, noApi)).not.toThrow();
    pull.base.ref = 'main';
    expect(() => checkBranchPolicy(pull, repository, noApi)).toThrow('Main accepts only');
  });
  it('rejects a release name from a fork', () => {
    const pull = pr('main', 'release/1.0.0-alpha');
    pull.head.repo.full_name = 'fork/picto';
    expect(() => checkBranchPolicy(pull, repository, noApi)).toThrow('this repository');
  });
  it('allows a fully gated release before it has landed on main', () => {
    expect(() => checkBranchPolicy(pr('main', 'release/1.0.0-alpha'), repository, gate())).not.toThrow();
  });
  it.each([{ completed: false }, { missingArtifact: true }])('blocks an ungated release: %j', state => {
    expect(() => checkBranchPolicy(pr('main', 'release/1.0.0-alpha'), repository, gate(state))).toThrow();
  });
});
