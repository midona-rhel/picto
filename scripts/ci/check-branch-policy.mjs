import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { findReleaseGate } from './find-release-gate.mjs';

// Checks a pull request against the rules for its base branch and throws
// with the reason when it breaks one.
//
// - Into `dev`: a work branch is named by its purpose and its issue number.
//   A release branch `release/<version>` of this repository is accepted when
//   the version in its `package.json` is the same version and the Alpha Gate
//   has passed for its head commit, with the three packages. A release is
//   merged into `dev` and published from there.
// - Into `main`: nothing. `main` is not part of a release.
//
// `api` takes the path of a GitHub API endpoint and returns the parsed
// answer.
export function checkBranchPolicy(pr, repository, api) {
  if (pr.base.ref === 'main') {
    throw new Error('Main is not used. Target dev.');
  }
  if (pr.base.ref !== 'dev') return;
  const sameRepository = pr.head.repo?.full_name === repository;
  const version = pr.head.ref.match(/^release\/(\d+\.\d+\.\d+-(?:alpha|rc)(?:[.-]\d+)?)$/)?.[1];
  if (version) {
    if (!sameRepository) throw new Error('A release branch must be a branch of this repository.');
    const file = api(`repos/${repository}/contents/package.json?ref=${pr.head.sha}`);
    const packagedVersion = JSON.parse(Buffer.from(file.content, 'base64').toString('utf8')).version;
    if (packagedVersion !== version) throw new Error('Release branch and package version must match.');
    // The publication requires the commit to be in dev. This check runs
    // before the merge, so it requires only the gate and its packages.
    findReleaseGate({ repository, sha: pr.head.sha, version, tag: `v${version}`, requireDevAncestry: false }, api);
    return;
  }
  // The pull requests 55 to 76 are older than the rule about names and keep
  // the names they have.
  const bootstrap = sameRepository && pr.number >= 55 && pr.number <= 76;
  const maintenance = sameRepository && /^dependabot\/.+$/.test(pr.head.ref);
  if (!bootstrap && !maintenance && !/^(?:feature|bug|ui|perf|refactor|docs|test|chore|deps)\/\d+-[^/]+$/.test(pr.head.ref)) {
    throw new Error('Work branches into dev need a purpose and issue number, for example bug/123-fix.');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  checkBranchPolicy(event.pull_request, process.env.GITHUB_REPOSITORY, endpoint =>
    JSON.parse(execFileSync('gh', ['api', endpoint], { encoding: 'utf8', timeout: 30_000 })));
  console.log('Branch policy passed.');
}
