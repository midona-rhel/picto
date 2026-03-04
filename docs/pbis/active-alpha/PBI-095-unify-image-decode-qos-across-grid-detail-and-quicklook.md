# PBI-095: Unify image decode QoS across grid, detail, and quicklook

## Priority
P1

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Grid uses `ImageAtlas` with its own queue/concurrency/budgets:
   - `./src/components/image-grid/imageAtlas.ts`
2. Detail/QuickLook use separate decode queue logic:
   - `./src/components/image-grid/useImagePreloader.ts`
   - `./src/components/image-grid/DetailView.tsx`
   - `./src/components/image-grid/DetailWindow.tsx`
   - `./src/components/image-grid/QuickLook.tsx`
3. Additional loading queue exists in:
   - `./src/components/image-grid/imageLoadManager.ts`

## Problem
Multiple independent decode/load schedulers compete for decode/network resources and can cause contention, stalls, and inconsistent thumb/full promotion behavior.

## Scope
- `./src/components/image-grid/imageAtlas.ts`
- `./src/components/image-grid/useImagePreloader.ts`
- `./src/components/image-grid/imageLoadManager.ts`
- new shared scheduler: `./src/components/image-grid/mediaQosScheduler.ts`

## Implementation
1. Introduce one shared scheduler with explicit lanes:
   - `critical` (visible detail/quicklook)
   - `visible` (viewport grid tiles)
   - `prefetch` (near/far prefetch)
2. Enforce global heavy-codec concurrency caps (WebP/AVIF) and per-lane budgets.
3. Make all decode requests cancellable and priority-upgradable.
4. Route `ImageAtlas` and `useImagePreloader` through the same scheduler API.

## Acceptance Criteria
1. Grid + detail view no longer run competing decode queues.
2. Thumb-first/full-later behavior is consistent across all image surfaces.
3. Scroll and right-arrow navigation stutter is reduced under mixed codec workloads.

## Test Cases
1. Scroll grid while navigating detail view rapidly.
2. Mixed PNG/WebP/AVIF library with heavy prefetch load.
3. Cancel/scope-switch during in-flight decode workload.

## Risk
Medium-high. Scheduler unification touches multiple hot paths.

