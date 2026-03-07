# PBI-405: Frontend topology policy and CI guardrails

## Priority
P1

## Audit Status (2026-03-07)
Status: **Not Implemented**

Evidence:
1. `PBI-401` now defines the target frontend topology, but nothing currently prevents drift back into root files and catch-all folders.
2. New code can still land in `src/components/*` or root `src/` because there is no CI enforcement.
3. There is no current guard against new imports from deprecated legacy paths.

## Problem
The frontend can only stay clean if the folder rules are enforced mechanically. Without CI guardrails, topology work will regress as soon as feature work resumes.

## Scope
- Frontend repository policy
- CI/guard scripts
- Import/path boundary checks
- File-size ceilings for known hotspot files

## Implementation
1. Add a frontend topology guard script.
2. Fail CI if new root-level `src/` files/folders appear outside the approved set.
3. Fail CI if new domain code lands in deprecated legacy locations.
4. Fail CI if migrated features are imported through non-canonical legacy paths.
5. Add hotspot file-size ceilings with explicit override process.
6. Document the policy in a short contributor-facing note.

## Acceptance Criteria
1. CI blocks new root drift.
2. CI blocks new imports from deprecated legacy paths where migrations are complete.
3. Legacy growth is no longer possible by accident.
4. Reviewers have a simple written policy to point to.

## Test Cases
1. Add a fake root-level `src/foo.tsx` file -> guard fails.
2. Add a fake new import from a deprecated legacy path -> guard fails.
3. Normal feature changes within canonical paths still pass.

## Risk
Low-medium. Guardrails are straightforward, but path rules need to be staged so they do not block valid transitional work too early.
