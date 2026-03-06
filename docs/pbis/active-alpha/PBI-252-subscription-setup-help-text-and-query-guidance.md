# PBI-252: Subscription setup help text and query guidance

## Priority
P2

## Audit Status (2026-03-06)
Status: **Blocked (Subscription Workstream Deferred)**

Blocked Reason:
1. Subscription workstream is deferred by product direction for now.
2. Keep this PBI in backlog but do not execute until unblocked.

Evidence:
1. A user reported: "Might want to have a short note on how to set up subscriptions, idk what a query is on twitter for instance."
2. The subscription panel has no inline help, tooltips, or examples explaining what to enter for each site.
3. Different sites use different query formats (tags for Danbooru, URLs for Twitter, etc.) but this is not communicated.

## Problem
The subscription panel assumes users already know the query format for each supported site. There is no help text, placeholder examples, or documentation inline. New users don't know what to type and have no way to learn without external help.

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
