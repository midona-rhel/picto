# PBI-502: Renderer boundary collapse

## Priority
P0

## Problem
The renderer/backend seam is spread across `#desktop/api`, `src/platform/api.ts`, feature-local wrappers, and leftover shared controllers that mostly rename transport calls.

## Goal
Reduce the renderer/backend seam to one obvious boundary.

## Implementation
1. Keep `#desktop/api` as the only renderer import boundary.
2. Split `src/platform/api.ts` internally into typed core commands, host APIs, and minimal normalizers.
3. Delete pass-through wrappers that only rename `api.*`.
4. Allow feature-local backend helpers only when they add real feature policy.

## Acceptance Criteria
1. Renderer code does not import backend transport directly outside `#desktop/api`.
2. Shared controller junk-drawer patterns are removed.
3. Feature-local transport helpers live with the feature they serve.
