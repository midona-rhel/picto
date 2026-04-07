# PBI-529: Frontend error feedback system

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The assessment is based on searching the frontend for error handling patterns. If errors are genuinely rare in practice (Rust core is local and reliable), the user impact may be low. However, disk-full, permission-denied, and corrupt-library scenarios do occur and currently produce no user-visible feedback.

## Priority
P1

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: Frontend API call failures are caught via `try/catch` and logged to `console.error` — the user sees nothing. No toast/notification/snackbar system exists. The `AppErrorBoundary` handles React render crashes but not async API errors. The `domainStore` has an 8-second stuck-fetch timeout that silently falls back to stale data. Searched for `notifications`, `toast`, `snackbar` across `src/` — zero results.

## Problem
When backend operations fail (import error, disk full, permission denied, corrupt file, database error), the frontend swallows the error and the user gets no feedback. The UI simply stops updating or shows stale data.

This creates a confusing experience: the user clicks "import" and nothing happens, or tags a file and the tag doesn't appear, with no indication of why.

## Scope
- `src/platform/**` — API call error handling
- `src/state/domainStore.ts` — stuck-fetch timeout
- `src/state/runtimeSyncStore.ts` — event handling errors
- New: toast/notification UI component
- New: error context provider

## Implementation
1. **Add Mantine Notifications**: Mantine provides `@mantine/notifications` with `notifications.show()`. Install and wire into the app shell (`App.tsx` needs `<Notifications />` provider).
2. **Create `showErrorToast(title, message)` utility**: Centralized error display function in `src/shared/lib/errorToast.ts`. Styled to match the existing design system (use `--color-negative` for error tint).
3. **Wire API error path**: In `api.ts`, catch errors from `invoke()` and call `showErrorToast()`. For structured errors (PBI-525), use the error code to select appropriate messaging. Until PBI-525 is done, show the raw error string.
4. **Wire stuck-fetch recovery**: When `domainStore` falls back to stale data after timeout, show a warning toast ("Sidebar refresh timed out — showing cached data").
5. **Wire import errors**: Import pipeline already returns error counts — surface non-zero error counts as a summary toast after bulk import completes.

## Acceptance Criteria
1. Failed API calls display a toast notification with error details.
2. Toasts auto-dismiss after 5 seconds but can be manually closed.
3. Import errors show a summary toast with counts (imported/skipped/errors).
4. Sidebar stuck-fetch shows a warning toast.
5. Toasts do not interrupt the user's workflow (non-modal, positioned in corner).

## Test Cases
1. Simulate API error (corrupt command name) → toast appears with error message.
2. Import a batch with some unsupported files → summary toast shows "3 imported, 2 skipped, 1 error".
3. Force sidebar timeout (mock slow backend) → warning toast appears.
4. Multiple errors in rapid succession → toasts stack without overlapping.
5. User dismisses toast → it disappears immediately.

## Risk
Low. Mantine Notifications is a well-tested library. The main risk is over-notifying — showing toasts for every minor issue creates alert fatigue. The implementation should filter by severity: only show toasts for errors the user can act on or needs to know about.
