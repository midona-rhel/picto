# PBI-507: Tags domain rewrite

## Priority
P0

## Problem
Tags still blur local semantics, ingest coercion, legacy naming, and renderer-side reparsing into one overgrown domain.

## Goal
Rebuild tags around one truthful core.

## Implementation
1. Keep canonical storage as `(namespace, value)`.
2. Keep local parser and ingest parser backend-owned only.
3. Make alias and implication semantics backend-owned.
4. Keep ingest coercion inside import and subscription adapters only.
5. Stop frontend reparsing or second-guessing stored tags.
6. Remove UI-triggered maintenance work.
7. Keep PTR out of local tag semantics.

## Acceptance Criteria
1. One backend tag service owns normalization, relations, merge, and compiler updates.
2. Frontend namespace logic is limited to display and input helpers.
