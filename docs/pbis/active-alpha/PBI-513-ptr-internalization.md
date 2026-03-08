# PBI-513: PTR internalization

## Priority
P0

## Problem
PTR still lingers as a visible product concept even though it is dormant and should not shape live user-facing behavior.

## Goal
Seal PTR as internal-only secondary tag data.

## Implementation
1. Remove PTR from settings, runtime UI, tags UI, and active product docs.
2. Keep backend persistence only if required for dormant implementation.
3. Treat PTR conceptually as `secondary_tag_db`.
4. Do not expose product-facing commands or UI paths for it.

## Acceptance Criteria
1. PTR is invisible to normal application behavior.
2. Only internal appendix docs mention it.
