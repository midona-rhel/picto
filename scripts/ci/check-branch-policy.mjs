import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { findReleaseGate } from './find-release-gate.mjs';

export function checkBranchPolicy(pr, repository, api) {
  if (pr.base.ref === 'main') {
    const version = pr.head.ref.match(/^release\/(\d+\.\d+\.\d+-(?:alpha|rc)(?:[.-]\d+)?)$/)?.[1];
    if (!version || pr.head.repo?.full_name !== repository) {
      throw new Error('Main accepts only release/<alpha-or-rc-version> from this repository. Target dev for development.');
    }
    const file = api(`repos/${repository}/contents/package.json?ref=${pr.head.sha}`);
    const packagedVersion = JSON.parse(Buffer.from(file.content, 'base64').toString('utf8')).version;
    if (packagedVersion !== version) throw new Error('Release branch and package version must match.');
    // Publication still requires main ancestry; the pre-merge check requires
    // the same exact successful package gate before the release lands there.
    findReleaseGate({ repository, sha: pr.head.sha, version, tag: `v${version}`, requireMainAncestry: false }, api);
  } else if (pr.base.ref === 'dev') {
    // These existing PRs predate issue-number names. Preserve their identities
    // during the transition, not a wildcard naming exception for future work.
    const sameRepository = pr.head.repo?.full_name === repository;
    const bootstrap = sameRepository && pr.number >= 55 && pr.number <= 76;
    const maintenance = sameRepository && /^(?:dependabot\/.+|main)$/.test(pr.head.ref);
    if (!bootstrap && !maintenance && !/^(?:feature|bug|ui|perf|refactor|docs|test|chore|deps)\/\d+-[^/]+$/.test(pr.head.ref)) {
      throw new Error('Work branches into dev need a purpose and issue number, for example bug/123-fix.');
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  checkBranchPolicy(event.pull_request, process.env.GITHUB_REPOSITORY, endpoint =>
    JSON.parse(execFileSync('gh', ['api', endpoint], { encoding: 'utf8', timeout: 30_000 })));
  console.log('Branch policy passed.');
}
