# PBI-227: First-run onboarding and library creation guidance

## Priority
P1

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A new user launched the app and immediately tried to download/import content without realizing a library must be created first.
2. The user had to be manually told "need to be in a library to start" with a screenshot.
3. Eagle shows a clear onboarding wizard on first launch that walks users through library creation.

## Problem
First-time users have no guidance on the required first step: creating a library. They land in an empty state with no indication of what to do, leading to confusion and failed workflows (downloads, imports, subscriptions all require an active library).

## Scope
- First-launch detection (no libraries exist)
- Welcome / onboarding screen or modal
- Library creation flow surfaced prominently

## Implementation
1. On startup, detect if no library exists (or no library is active).
2. Show a welcome screen/modal with a clear call-to-action: "Create your first library" with a name and location picker.
3. After library creation, show a brief orientation pointing to key areas: sidebar navigation, download/subscription button, import via drag-and-drop.
4. Optionally offer a "quick start" path: create library + start first subscription in one guided flow.

## Acceptance Criteria
1. First launch with no libraries shows a welcome/onboarding screen.
2. User cannot accidentally bypass library creation and end up in a broken empty state.
3. After library creation, user lands in the library with enough context to start using the app.
4. Subsequent launches skip onboarding and go directly to the last active library.

## Test Cases
1. Fresh install, first launch — onboarding screen appears.
2. Complete onboarding — lands in new library, ready to use.
3. Second launch — goes straight to library, no onboarding.
4. Delete all libraries, relaunch — onboarding reappears.

## Risk
Low. Primarily UI work with simple first-launch detection logic.
