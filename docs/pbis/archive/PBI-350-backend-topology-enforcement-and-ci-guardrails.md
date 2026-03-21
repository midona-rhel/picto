# PBI-350: Backend topology enforcement and CI guardrails

## Priority
P1

## Archive Status (2026-03-18)
Status: **Rejected**

Rejected Reason:
1. Product direction explicitly rejected adding more topology/process guardrails.
2. The remaining backend cleanup work should land through direct code simplification, not new CI or policy enforcement.

## Original Problem
Even a good backend restructure will regress if there are no guardrails preventing new flat-root files, lingering imports from deleted paths, or temporary shims that never get removed.

## Original Scope
- Backend only
- CI and policy guardrails for `core/src/**`

## Original Implementation
1. Fail CI if unapproved new root-level Rust files are added.
2. Fail CI if removed legacy module paths are imported again.
3. Add a `LEGACY:` marker convention for temporary compatibility paths.
4. Add backend restructure PR requirements: canonical owner, deleted path, lines deleted, remaining shim owner.
5. Optionally add soft line-count ceilings for backend monoliths.

## Original Acceptance Criteria
1. The backend topology is mechanically enforced in CI.
2. Legacy paths cannot silently re-enter the codebase.
3. Temporary compatibility code is visible and time-bounded.
4. Backend cleanup becomes an enforced process, not a best-effort habit.
