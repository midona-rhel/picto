# PBI-511: Inspector and metadata consolidation

## Priority
P1

## Problem
Metadata behavior is still spread across several hooks and UI fragments, and the renderer does too much interpretation work.

## Goal
Centralize metadata as backend-owned read models and keep inspector hooks thin.

## Implementation
1. Backend returns selection and metadata DTOs ready for display.
2. Centralize notes, source URLs, rating, colors, file facts, and collection summary.
3. Add missing “site time” style metadata only if retained in the product model.
4. Keep frontend inspector hooks as fetch/mutate helpers only.

## Acceptance Criteria
1. Inspector does not recompute domain logic.
2. Single, multi, and collection selection stay coherent.
