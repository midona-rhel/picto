# PBI-163: Standardize runtime/product namespace (`picto` vs `imaginator`)

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

## Problem
The codebase uses both `picto` and `imaginator` identifiers across package names, crates, global window namespace, and runtime channels. This causes onboarding ambiguity and brittle scripts.

## Scope
- `/Users/midona/Code/imaginator/package.json`
- `/Users/midona/Code/imaginator/core/Cargo.toml`
- `/Users/midona/Code/imaginator/native/picto-node/Cargo.toml`
- `/Users/midona/Code/imaginator/electron/*`
- `/Users/midona/Code/imaginator/src/desktop/*`

## Implementation
1. Decide one canonical runtime namespace.
2. Apply consistently to:
   - package/crate names
   - preload global key
   - IPC channel prefixes
   - documentation and scripts.
3. Provide temporary typed alias only where migration is required.
4. Add guard script to prevent new mixed namespace usage.

## Acceptance Criteria
1. New contributors see one product/runtime namespace only.
2. No mixed channel/global naming in active runtime paths.
3. All scripts and CI pass after rename/alias cutover.

## Test Cases
1. App boots and IPC commands/events flow with canonical namespace.
2. Existing controller invocations still resolve after cutover.

## Risk
Medium. Wide rename blast radius; stage carefully.

