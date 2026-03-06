# PBI-121: Custom thumbnail management

## Priority
P2

## Audit Status (2026-03-03)
Status: **Partially Implemented**

Evidence:
1. Thumbnail regeneration is implemented (`mod+shift+t`) through `src/components/image-grid/hooks/useGridHotkeys.ts` and backend regenerate endpoints.
2. Missing from this PBI: custom thumbnail from file/clipboard, reset custom thumbnail, per-item thumbnail background options, and set-video-thumbnail-from-current-frame.

## Problem
Users cannot set custom thumbnails, refresh broken thumbnails, or control thumbnail appearance.

## Scope
- Backend: custom thumbnail storage and retrieval
- Context menu additions
- `src/lib/shortcuts.ts` — Cmd+Alt+R (refresh thumbnail)

## Implementation
1. Custom Thumbnail from File: pick an image file to use as thumbnail.
2. Custom Thumbnail from Clipboard: paste clipboard content as thumbnail.
3. Reset Custom Thumbnail: revert to auto-generated.
4. Refresh Thumbnail (Cmd+Alt+R): regenerate from source file.
5. Thumbnail Background: per-item setting (none/white/black/gray/checkerboard grid).
6. Video thumbnail: capture current frame (ties into video frame-capture capabilities).

## Acceptance Criteria
1. Right-click → Custom Thumbnail → Select File works.
2. Refresh Thumbnail regenerates from source.
3. Thumbnail background options render correctly in grid.
4. Custom thumbnails persist across sessions.

## Test Cases
1. Set custom thumbnail from file — grid shows new thumbnail.
2. Reset — original auto-generated thumbnail restored.
3. Cmd+Alt+R on broken thumbnail — thumbnail regenerates.
4. Set background to black — dark padding behind thumbnail in grid.

## Risk
Low-Medium. Thumbnail storage exists; needs override mechanism.
