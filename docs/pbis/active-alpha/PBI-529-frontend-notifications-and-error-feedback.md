# PBI-529: Frontend notifications and error feedback

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The assessment is based on searching the frontend for error handling patterns. If errors are genuinely rare in practice (Rust core is local and reliable), the user impact may be low. However, disk-full, permission-denied, and corrupt-library scenarios do occur and currently produce no user-visible feedback.

## Priority
P1

## Audit Status (2026-08-07)
Status: **Not Implemented**

The app still lacks one non-modal notification path for operation results and actionable errors.
Duplicate scan results were previously rendered over the comparison workspace; that local banner
has been removed and belongs here instead.

## Problem
When backend operations fail (import error, disk full, permission denied, corrupt file, database error), the frontend swallows the error and the user gets no feedback. The UI simply stops updating or shows stale data.

This creates a confusing experience: the user clicks "import" and nothing happens, or tags a file and the tag doesn't appear, with no indication of why.

## Scope
- `src/platform/**` — API call error handling
- `src/runtime/**` — runtime settlement failures
- `src/shared/lib/notifications.ts` — shared notification helpers
- `src/entrypoints/main.tsx` — notification host

## Implementation
1. **Use Mantine Notifications**: keep the existing `<Notifications />` host in `src/entrypoints/main.tsx` and the helpers in `src/shared/lib/notifications.ts`.
2. **Keep transport errors normalized**: `src/platform/ipc.ts` removes Electron wrapper text. The operation owner supplies the user-facing title and decides whether the error is actionable enough to notify.
3. **Wire active operation failures**: controllers and screens that currently log or swallow import, mutation, and runtime-settlement failures use the shared notification helpers. Do not globally toast every IPC rejection.
4. **Wire stale-data recovery**: when an active runtime settle path falls back to cached data after a timeout, show one warning that identifies the stale surface.
5. **Wire import errors**: Import pipeline already returns error counts — surface non-zero error counts as a summary toast after bulk import completes.
6. **Wire duplicate scan results**: After an explicit duplicate scan, show one non-modal summary:
   `Found N new review pairs` or `Scan complete — no new review pairs`. The duplicate comparison
   screen must only reload its queue; it must not render scan-result banners over media.
7. **Own all duplicate-review operation notifications**:
   - queue, metadata, pagination, scan, and resolution failures use the shared error notification
   - ambiguous smart merge uses one warning telling the user to choose a side or keep both
   - successful ordinary decisions do not notify; the queue advancing is sufficient feedback
   - no transient duplicate-review message may alter or cover the comparison layout

## Acceptance Criteria
1. Failed API calls display a toast notification with error details.
2. Toasts auto-dismiss after 5 seconds but can be manually closed.
3. Import errors show a summary toast with counts (imported/skipped/errors).
4. Sidebar stuck-fetch shows a warning toast.
5. Toasts do not interrupt the user's workflow (non-modal, positioned in corner).
6. Duplicate scan results use the shared notification path and never cover comparison controls or media.
7. Duplicate-review failures and ambiguous smart merges use the shared notification path; ordinary successful decisions remain silent.

## Test Cases
1. Simulate API error (corrupt command name) → toast appears with error message.
2. Import a batch with some unsupported files → summary toast shows "3 imported, 2 skipped, 1 error".
3. Force sidebar timeout (mock slow backend) → warning toast appears.
4. Multiple errors in rapid succession → toasts stack without overlapping.
5. User dismisses toast → it disappears immediately.
6. Re-scan duplicates with and without new pairs → one accurate summary notification appears and the comparison layout does not move.
7. Force each duplicate-review API failure and an ambiguous smart merge → shared notifications appear with no local banner or layout shift.

## Risk
Low. Mantine Notifications is a well-tested library. The main risk is over-notifying — showing toasts for every minor issue creates alert fatigue. The implementation should filter by severity: only show toasts for errors the user can act on or needs to know about.
