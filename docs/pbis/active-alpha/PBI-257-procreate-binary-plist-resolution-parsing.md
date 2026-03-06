# PBI-257: Procreate binary plist resolution parsing

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. `/Users/midona/Code/imaginator/core/src/files/specialty.rs` returns unsupported for most Procreate files due binary plist parsing missing.
2. Code includes `TODO: Add plist crate for binary plist support`.

## Problem
`.procreate` files commonly use binary plist in `Document.archive`, but current logic only attempts XML parsing. This causes resolution extraction to fail for real-world files.

## Scope
- `/Users/midona/Code/imaginator/core/src/files/specialty.rs`
- Add binary plist decoding support

## Implementation
1. Add binary plist parser dependency.
2. Parse `Document.archive` for canvas width/height and orientation fields.
3. Keep XML fallback for legacy/edge files.
4. Ensure errors degrade gracefully and do not block import.

## Acceptance Criteria
1. Typical modern Procreate files return valid resolution.
2. XML plist fallback still works.
3. Unsupported/corrupt plist fails safely with actionable error.

## Test Cases
1. Binary plist Procreate sample → width/height extracted.
2. XML plist sample → fallback parser still succeeds.
3. Missing `Document.archive` entry → controlled error path.

## Risk
Medium. Binary plist key layout may vary by app version; parser needs version-tolerant field lookup.
