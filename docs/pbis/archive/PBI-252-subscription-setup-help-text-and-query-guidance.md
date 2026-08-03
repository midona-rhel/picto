# PBI-252: Subscription setup help text and query guidance

## Priority
P2

## Audit Status (2026-03-08)
Status: **Partially Implemented**

Evidence:
1. The subscription workstream is active; the previous "deferred" blocker is stale.
2. The add-query UI in [src/features/subscriptions/components/SubscriptionGroupsPanel.tsx](./src/features/subscriptions/components/SubscriptionGroupsPanel.tsx) still uses a generic `placeholder="Query"` with no per-site guidance.
3. There is still no inline help icon, tooltip, or expandable explainer showing example query formats for different sites.
4. There is some auth guidance now: when adding a query to a site that supports auth and lacks credentials, the UI shows an informational notification directing the user to configure credentials.

## Problem
The remaining gap is query guidance, not subscription enablement itself. Users can create and run subscriptions, but the add-query form still assumes they already know each site's query syntax.

## Scope
- Subscription panel download input area
- Per-site placeholder text and help tooltips

## Implementation
1. Add **placeholder text** to the query input field that changes based on the selected site:
   - Danbooru: `e.g. "1girl blue_hair" or paste a post URL`
   - e621: `e.g. "wolf rating:safe" or paste a post URL`
   - Twitter/X: `e.g. paste a tweet URL or profile URL`
   - Rule34.xxx: `e.g. "character_name" or paste a post URL`
2. Add a small **help icon** (?) next to the input that opens a tooltip or expandable section explaining:
   - What format the query should be in
   - Example queries for the selected site
   - Whether authentication is required
3. For sites that require auth, show a brief note: "This site requires credentials — configure in the panel above."

## Acceptance Criteria
1. Each site shows relevant placeholder text in the query input.
2. Help icon/tooltip explains the query format with examples.
3. Sites requiring auth show a note directing to the auth panel.
4. A new user can set up a subscription without external help.

## Test Cases
1. Select Danbooru — placeholder shows tag-based example.
2. Select Twitter — placeholder shows URL-based example.
3. Click help icon — tooltip with detailed examples appears.
4. Select a site requiring auth with no credentials — note appears.

## Risk
Low. Static UI text changes. No backend work.
