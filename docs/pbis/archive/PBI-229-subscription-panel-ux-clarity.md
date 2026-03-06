# PBI-229: Subscription panel UX clarity

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user was confused by the subscription UI, not understanding that the top panel is for auth configuration and the bottom panel is where you actually add downloads.
2. Confirmed internally: "need to make it clearer — you work in the bottom window, top is only for auth config."
3. The split layout provides no labels, headers, or visual hierarchy to communicate the purpose of each section.

## Problem
The subscription/download panel has a split layout where the top section handles site authentication and the bottom section is the actual download interface. This distinction is not visually communicated, causing new users to interact with the wrong panel and get stuck.

## Scope
- `src/components/` — subscription/download panel layout
- Visual hierarchy, labels, and section headers

## Implementation
1. Add clear section headers: e.g. "Site Accounts" (top) and "Downloads" or "Add Subscription" (bottom).
2. Visually differentiate the two sections with distinct backgrounds, dividers, or card styling.
3. Consider collapsing the auth section by default if credentials are already configured, so the download interface is immediately prominent.
4. Add placeholder/helper text in the download section: e.g. "Paste a URL or search by tag to start downloading."
5. For sites that don't require auth, hide or grey out the auth section entirely.

## Acceptance Criteria
1. Each section has a visible header communicating its purpose.
2. New users can identify the download input area without external guidance.
3. Auth section collapses or de-emphasizes when credentials are already saved.
4. Sites not requiring auth don't show an empty auth panel.

## Test Cases
1. Open subscription panel with no credentials — both sections visible, clearly labeled.
2. Open subscription panel with Danbooru credentials saved — auth section collapsed/minimal, download section prominent.
3. New user opens panel — can locate download input within 5 seconds without external help.
4. Select a site that needs no auth (Danbooru) — auth section hidden or clearly marked as optional.

## Risk
Low. Primarily layout and labeling changes with no backend work.
