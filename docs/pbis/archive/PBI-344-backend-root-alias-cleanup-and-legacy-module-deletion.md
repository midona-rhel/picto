# PBI-344: Backend root alias cleanup and legacy module deletion

## Priority
P1

## Audit Status (2026-03-15)
Status: **Implemented**

Evidence:
1. `core/src/lib.rs` now exposes canonical top-level modules directly without old flat root aliases.
2. The root no longer contains legacy domain files like the pre-folderization controller/module layout.
3. Grepping for old flat backend module paths no longer finds stale imports or compatibility aliases.

## Problem
A backend reorganization is not complete until the old root-level aliases are removed. Otherwise the codebase will end up supporting both the old and new architecture simultaneously.

Reference architecture: `docs/rust-core-rearchitecture-blueprint-2026-03-07.md`

## Scope
- `core/src/lib.rs`
- temporary re-exports and compatibility aliases
- obsolete root-level files after migration

## Implementation
1. Track temporary aliases introduced during folderization.
2. Remove them once all imports are migrated.
3. Delete obsolete root-level files.
4. Ensure the final backend tree has one canonical path per module.

## Acceptance Criteria
1. No obsolete root-level aliases remain.
2. No duplicate old/new module paths remain.
3. The final tree has one canonical ownership path for each backend subsystem.

## Test Cases
1. Build/tests pass with old aliases removed.
2. `rg "crate::(old path)" core/src` returns no stale imports for migrated modules.

## Risk
Low-medium. Cleanup-oriented but necessary to prevent architectural drift.
