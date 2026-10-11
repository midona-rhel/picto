#!/usr/bin/env bash
# Configure the GitHub repository for the dev -> release/<version> -> main flow.
# Idempotent: re-running updates labels and rulesets in place.
#
#   scripts/github/setup-repository.sh            # midona-rhel/picto
#   REPO=owner/name scripts/github/setup-repository.sh
#
# Requires `gh` authenticated as a repository admin, and `jq`.
set -euo pipefail

REPO="${REPO:-midona-rhel/picto}"
api() { gh api -H "Accept: application/vnd.github+json" "$@"; }

echo "== dev branch"
if ! api "repos/$REPO/branches/dev" >/dev/null 2>&1; then
  main_sha="$(api "repos/$REPO/git/ref/heads/main" --jq .object.sha)"
  api -X POST "repos/$REPO/git/refs" -f ref=refs/heads/dev -f sha="$main_sha" >/dev/null
  echo "created dev at $main_sha"
fi

echo "== repository settings"
# Squash for work PRs into dev (one commit per issue); merge commits for
# release PRs into main so each released commit stays in main's ancestry,
# which scripts/ci/find-release-gate.mjs requires before publishing.
api -X PATCH "repos/$REPO" \
  -f default_branch=dev \
  -F has_issues=true \
  -F allow_squash_merge=true \
  -F allow_merge_commit=true \
  -F allow_rebase_merge=false \
  -F allow_auto_merge=true \
  -F delete_branch_on_merge=true \
  -f squash_merge_commit_title=PR_TITLE \
  -f squash_merge_commit_message=BLANK \
  -f merge_commit_title=PR_TITLE \
  -f merge_commit_message=PR_BODY >/dev/null

echo "== labels"
label() {
  api -X POST "repos/$REPO/labels" -f name="$1" -f color="$2" -f description="$3" >/dev/null 2>&1 \
    || api -X PATCH "repos/$REPO/labels/$(jq -rn --arg v "$1" '$v|@uri')" -f color="$2" -f description="$3" >/dev/null
}
label "priority: P0" b60205 "Data loss, crash on open, or release blocker"
label "priority: P1" d93f0b "Broken core workflow; fix in the next release"
label "priority: P2" fbca04 "Default priority"
label "priority: P3" c5def5 "Nice to have"
label "type: bug" d73a4a "Something is broken"
label "type: feature" a2eeef "New user-facing capability"
label "type: refactor" 5319e7 "Structure change without intended behavior change"
label "type: perf" 0e8a16 "Latency, memory, or throughput"
label "type: docs" 0075ca "Documentation only"
label "type: chore" ededed "Build, CI, dependencies, process"
label "area: library" 1d76db "picto-library: schema, mutations, projections"
label "area: core" 1d76db "core: runtime, queues, IPC, media processing"
label "area: sources" 1d76db "picto-sources: site adapters and HTTP"
label "area: renderer" 1d76db "React UI"
label "area: electron" 1d76db "Electron shell, updater, packaging"
label "status: triage" ffffff "Needs evidence or a priority"
label "status: blocked" 000000 "Waiting on something outside the issue"
label "schema change" b60205 "Touches schema_v1.sql or migrations"

echo "== rulesets"
upsert_ruleset() {
  local name="$1" body="$2" id
  id="$(api "repos/$REPO/rulesets" --jq ".[] | select(.name == \"$name\") | .id" || true)"
  if [ -n "$id" ]; then
    api -X PUT "repos/$REPO/rulesets/$id" --input - <<<"$body" >/dev/null
    echo "updated $name"
  else
    api -X POST "repos/$REPO/rulesets" --input - <<<"$body" >/dev/null
    echo "created $name"
  fi
}

checks='[
  {"context": "verify / verify:renderer", "integration_id": 15368},
  {"context": "verify / verify:rust", "integration_id": 15368},
  {"context": "branch-policy", "integration_id": 15368}
]'

branch_ruleset() { # name ref merge_method
  jq -n --arg name "$1" --arg ref "$2" --arg method "$3" --argjson checks "$checks" '{
    name: $name, target: "branch", enforcement: "active",
    conditions: {ref_name: {include: [$ref], exclude: []}},
    bypass_actors: [],
    rules: [
      {type: "deletion"},
      {type: "non_fast_forward"},
      {type: "pull_request", parameters: {
        required_approving_review_count: 0,
        dismiss_stale_reviews_on_push: true,
        require_code_owner_review: false,
        require_last_push_approval: false,
        required_review_thread_resolution: true,
        allowed_merge_methods: ($method | split(","))}},
      {type: "required_status_checks", parameters: {
        strict_required_status_checks_policy: true,
        required_status_checks: $checks}}
    ]}'
}

upsert_ruleset "dev: reviewed squash merges" "$(branch_ruleset "dev: reviewed squash merges" refs/heads/dev squash,merge)"
upsert_ruleset "main: releases only" "$(branch_ruleset "main: releases only" refs/heads/main merge)"

# Release branches are the release record and must still exist when the tag
# publishes (find-release-gate.mjs reads refs/heads/release/<version>). The
# deletion rule also stops auto-delete-on-merge from removing them.
upsert_ruleset "release branches" "$(jq -n '{
  name: "release branches", target: "branch", enforcement: "active",
  conditions: {ref_name: {include: ["refs/heads/release/*"], exclude: []}},
  bypass_actors: [],
  rules: [{type: "deletion"}, {type: "non_fast_forward"}]}')"

# Keep creation bypass separate from immutable-tag rules.
upsert_ruleset "release tags" "$(jq -n '{
  name: "release tags", target: "tag", enforcement: "active",
  conditions: {ref_name: {include: ["refs/tags/v*"], exclude: []}},
  bypass_actors: [],
  rules: [{type: "deletion"}, {type: "update"}]}')"
upsert_ruleset "release tag creation" "$(jq -n '{
  name: "release tag creation", target: "tag", enforcement: "active",
  conditions: {ref_name: {include: ["refs/tags/v*"], exclude: []}},
  bypass_actors: [{actor_id: 5, actor_type: "RepositoryRole", bypass_mode: "always"}],
  rules: [{type: "creation"}]}')"

echo "done: default branch is dev; main accepts release/<version> PRs only."
