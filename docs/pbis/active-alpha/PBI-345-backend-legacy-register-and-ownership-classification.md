# PBI-345: Backend legacy register and ownership classification

## Priority
P1

## Problem
The backend has architecture plans, but no canonical register that says which files are canonical, transitional, merge-legacy, or delete-legacy. Without that, cleanup remains subjective and legacy survives by default.

## Scope
- Backend only: `core/src/**`
- Produce and maintain a register with file-group classification, target owner, and removal condition

## Implementation
1. Create and maintain `docs/backend-legacy-register-2026-03-07.md`.
2. Classify all backend file groups as `canonical`, `transitional`, `legacy-merge`, or `legacy-delete`.
3. Record target owner and removal condition for each legacy path.
4. Cross-link the register from `PBI-240`, `PBI-233`, and the deletion-program PBIs.

## Acceptance Criteria
1. The register covers every backend file group.
2. High-confidence delete-now candidates are explicitly listed.
3. Merge-then-delete candidates are explicitly listed.
4. New backend cleanup work uses the register as the source of truth.
