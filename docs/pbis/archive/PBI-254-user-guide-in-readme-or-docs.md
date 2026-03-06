# PBI-254: User guide — getting started and basic usage documentation

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. The README contains only development setup, quality gates, and packaging instructions — no user-facing documentation.
2. Multiple testers across sessions have needed manual hand-holding to understand basic workflows: creating a library, importing files, setting up subscriptions, using the inbox, creating collections.
3. There is no user guide, quick start, or feature overview anywhere in the repository or shipped with the application.

## Problem
New users have no documentation explaining how to use Picto. Every tester so far has required real-time guidance via chat to figure out basic flows. The README is developer-only. There is no:
- Getting started guide (create library → import first images → browse)
- Feature overview (what is a subscription, what is the inbox, what are collections)
- Workflow documentation (how to triage inbox, how to organize into folders, how to use tags)

## Scope
- `README.md` — add a "Getting Started" section or link to a user guide
- `docs/guide/` (or similar) — detailed user documentation

## Implementation

### Quick start in README
Add a "Getting Started" section to the README with the minimum steps:
1. Launch Picto
2. Create a library (name + location)
3. Import images (drag-and-drop or import button)
4. Browse your library

### Full user guide in docs/
Create `docs/guide/` with markdown files covering:

1. **getting-started.md** — Library creation, first import, navigating the grid
2. **importing.md** — Drag-and-drop, import button, supported file formats
3. **subscriptions.md** — What subscriptions are, how to set up a download from Danbooru/e621/etc., auth configuration, query format per site
4. **inbox.md** — What the inbox is, how to triage (accept/reject), keyboard shortcuts
5. **folders.md** — Creating folders, organizing images, subfolder hierarchy
6. **collections.md** — What collections are, how to create them, reordering
7. **tags.md** — Tag system overview, adding/removing tags, namespaces, searching by tag
8. **search.md** — How search works, what fields are searchable, search syntax
9. **settings.md** — Key settings and what they do

### In-app reference (future)
Optionally link to the guide from a Help menu or from empty states (e.g. empty library → "Get started" link).

## Acceptance Criteria
1. README has a "Getting Started" section that a new user can follow.
2. A user guide exists in `docs/guide/` covering the core workflows.
3. A new tester can create a library, import files, and browse without external help.
4. Subscription setup guide includes per-site query format examples.

## Test Cases
1. New user follows README getting started → successfully creates library and imports files.
2. User reads subscriptions guide → sets up a Danbooru download without chat support.
3. User reads inbox guide → understands accept/reject flow.

## Risk
Low. Pure documentation. Should be written by someone who understands the intended workflows (not reverse-engineered from code).
