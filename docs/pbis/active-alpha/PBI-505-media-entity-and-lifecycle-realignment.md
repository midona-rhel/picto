# PBI-505: Media entity and lifecycle realignment

## Priority
P0

## Problem
The actual core domain is obscured by historical naming and side-rules around files, inbox, trash, and grouped media.

## Goal
Define the actual library item and lifecycle model cleanly.

## Implementation
1. `MediaEntity` is the logical item for image, video, PDF, document, or other file-backed content.
2. Lifecycle is only `inbox | active | trash`.
3. Collection membership is separate from lifecycle.
4. Collection members are hidden from general scopes through backend query rules.
5. Import and status mutation paths emit truthful lifecycle mutation receipts.

## Acceptance Criteria
1. No fourth collection lifecycle state exists.
2. Sidebar, grid, and inspector agree on lifecycle and collection-member visibility.
