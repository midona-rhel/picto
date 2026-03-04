# PBI-109: Random view with refresh

## Priority
P2

## Audit Status (2026-03-03)
Status: **Not Implemented**

Evidence:
1. Eagle has Cmd+5 for Random view and R key to refresh the random selection.
2. Picto has no random view.

## Problem
Users cannot browse a random selection of their library for inspiration.

## Scope
- `src/stores/navigationStore.ts` — add random view type
- `src/components/sidebar/Sidebar.tsx` — random sidebar item
- Backend: new dispatch command or client-side random sampling

## Implementation
1. Add "Random" as a navigation view type.
2. On navigation to Random, fetch N random file hashes from the database.
3. R key re-rolls the random selection.
4. Display count in sidebar.

## Acceptance Criteria
1. Cmd+5 navigates to Random view showing ~50 random images.
2. R key refreshes with new random selection.
3. Selection changes each visit.

## Test Cases
1. Navigate to Random — shows random images, different each time.
2. Press R — new set of random images loads.

## Risk
Low. Simple random query + frontend view.
