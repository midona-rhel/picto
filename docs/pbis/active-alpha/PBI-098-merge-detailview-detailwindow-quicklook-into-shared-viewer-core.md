# PBI-098: Merge `DetailView`/`DetailWindow`/`QuickLook` into a shared viewer core

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Three separate image viewer implementations with overlapping behavior:
   - `./src/components/image-grid/DetailView.tsx` (505 lines)
   - `./src/components/image-grid/DetailWindow.tsx` (659 lines)
   - `./src/components/image-grid/QuickLook.tsx` (316 lines)
2. All three implement similar navigation, thumb/full swap, decode timing, and keyboard handling.

## Problem
Viewer behavior is duplicated across three components, which causes drift, repeated bug fixes, and inconsistent UX/performance tuning.

## Scope
- `./src/components/image-grid/DetailView.tsx`
- `./src/components/image-grid/DetailWindow.tsx`
- `./src/components/image-grid/QuickLook.tsx`
- new shared core under `./src/components/image-grid/viewer/`

## Implementation
1. Extract shared viewer state machine/hook:
   - index navigation
   - thumb->full crossfade policy
   - decode/prefetch policy hooks
   - keyboard mapping
2. Keep only shell-specific concerns in each surface (window chrome, modal framing, controls density).
3. Standardize transition timings and fallback behavior across all three.
4. Add shared tests for navigation/decode transition behavior.

## Acceptance Criteria
1. Viewer logic is centralized and reused across all three surfaces.
2. PNG/WebP thumb->full behavior is consistent everywhere.
3. Future fixes to decode/crossfade logic are made once.

## Test Cases
1. Rapid arrow navigation with thumb-first/full-later policy in all three viewers.
2. Spacebar peek behavior parity with detail window behavior.
3. Transparent image handling and no-black-frame transitions across surfaces.

## Risk
Medium. High-impact refactor with strong long-term reuse/perf payoff.

