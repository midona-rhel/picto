# PBI-101: Standardize media-card primitives across grid-adjacent views

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Multiple components build their own image-card UI patterns:
   - `/Users/midona/Code/imaginator/src/components/Collections.tsx`
   - `/Users/midona/Code/imaginator/src/components/DuplicateManager.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/GlassImagePreview.tsx`
   - `/Users/midona/Code/imaginator/src/components/image-grid/SubfolderGrid.tsx`
2. Card-level overlays, fallback placeholders, title/meta rows, and action affordances are duplicated.

## Problem
Media card behavior and style are fragmented, which causes inconsistent transitions and duplicates performance-sensitive rendering logic.

## Scope
- files listed above
- new primitives under `/Users/midona/Code/imaginator/src/components/ui/media-card/`

## Implementation
1. Create shared media-card primitives:
   - `MediaCardFrame`
   - `MediaCardImage`
   - `MediaCardOverlay`
   - `MediaCardMeta`
2. Standardize thumb/fallback rendering and alpha-background policy at the primitive level.
3. Adopt primitives in collections and duplicate manager first.
4. Keep view-specific controls as slots.

## Acceptance Criteria
1. Card visual and transition behavior is consistent across grid-adjacent views.
2. Placeholder/fallback/overlay logic is centralized.
3. Shared media-card components are reused by at least two major feature surfaces.

## Test Cases
1. Collections card rendering with missing thumb and loaded thumb.
2. Duplicate manager left/right media cards with overlay metadata.
3. Subfolder/preview cards maintain expected interaction behavior.

## Risk
Low to medium. Mostly compositional extraction with moderate surface area.

