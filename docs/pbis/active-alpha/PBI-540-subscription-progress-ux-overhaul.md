# PBI-540: Subscription progress UX overhaul

## Priority
P1

## Problem
The subscription progress display in the sidebar bottom (`SidebarJobStatus`) is minimal and inconsistent:

1. **Naming is inconsistent**: Some subscriptions show by query type, some by query text, some fall back to "Subscription {id}". No clear hierarchy.
2. **No phase visibility**: The user can't tell if gallery-dl is downloading, if files are being imported, or if collections are being assembled. Just a vague "status_text".
3. **Errors are invisible**: Gallery-dl crashes, auth failures, and import errors are only visible in stdout logs — not surfaced in the UI progress area.
4. **Scrollbar hides content**: When the subscription list overflows, the scrollbar overlaps the job cards instead of using a gutter.
5. **No spinner**: Running subscriptions show an indeterminate progress bar but no actual spinner indicator.
6. **No list of active downloads**: Can't see all concurrent subscriptions at a glance.

## Scope

### Backend (progress events)
- `core/src/subscriptions/sync_engine/mod.rs` — emit more granular progress phases
- `core/src/subscriptions/sync_engine/importing.rs` — emit collection materialization phase
- `core/src/subscriptions/run_orchestrator.rs` — emit group-level progress with subscription names

### Frontend (sidebar component)
- `src/features/layout/components/SidebarJobStatus.tsx` — redesign
- `src/features/layout/components/SidebarJobStatus.module.css` — restyle
- `src/features/subscriptions/subscriptionProgressStore.ts` — store richer progress data

## Design

### Two-row card layout per subscription

```
┌─────────────────────────────────────────────┐
│ ◉ Group Name › Query Type › query_text      │  ← row 1: hierarchy
│   Downloading file 3/27...                  │  ← row 2: current phase
└─────────────────────────────────────────────┘
```

**Row 1** (name hierarchy):
- Group name (e.g. "My Artists")
- Query display name or type (e.g. "onlyfans/f1nn5ter")
- Falls back gracefully: group → subscription name → query text

**Row 2** (phase + detail):
- Phases: `Starting gallery-dl...` → `Downloading file {n}...` → `Importing file {n}/{total}...` → `Creating collection "{name}"...` → `Completed` / `Failed: {reason}`
- Errors shown inline in red: `Error: gallery-dl exited with code 1 (unauthorized)`

### Progress phases to emit from backend

| Phase | Emitted from | status_text format |
|-------|-------------|-------------------|
| Starting | sync_engine/mod.rs | `Starting gallery-dl...` |
| Downloading | sync_engine/mod.rs (streaming loop) | `Downloading file {n}...` |
| Stashing | sync_engine/mod.rs (collection member) | `Stashing page {n} for collection...` |
| Importing | importing.rs (materialize_collection) | `Importing {n}/{total} files...` |
| Creating collection | importing.rs (materialize_collection) | `Creating collection "{name}" ({n} items)` |
| Completed | sync_engine/mod.rs | `Completed — {downloaded} new, {skipped} skipped` |
| Error | sync_engine/mod.rs | `Failed: {failure_kind}` or `Error: {message}` |

### Scrollbar gutter
- Add `scrollbar-gutter: stable` to the subscription list container
- Or use overlay scrollbar with padding-right compensation

### Spinner
- Add a small animated spinner (CSS-only) to the left of running subscriptions instead of / alongside the download icon

### Error surfacing
- When a subscription finishes with errors, show the last error message in the phase row (red text)
- Gallery-dl stderr errors should be captured and forwarded as part of the task detail

## Files to modify

| File | Change |
|------|--------|
| `core/src/subscriptions/sync_engine/mod.rs` | Emit granular phase progress (downloading/stashing/importing/creating) |
| `core/src/subscriptions/sync_engine/importing.rs` | Emit progress during `materialize_collection` |
| `core/src/subscriptions/run_orchestrator.rs` | Include group_name in task detail |
| `src/features/layout/components/SidebarJobStatus.tsx` | Redesign with two-row cards, spinner, error display |
| `src/features/layout/components/SidebarJobStatus.module.css` | Restyle with gutter scrollbar, spinner animation |
| `src/features/subscriptions/subscriptionProgressStore.ts` | Add `group_name`, `phase`, `last_error` fields |

## Acceptance Criteria
1. Running subscriptions show group name → query type → query text hierarchy.
2. Current phase (downloading/importing/creating collection) is visible in real time.
3. Errors are shown inline in the progress card (red text, last error message).
4. Scrollbar doesn't overlap card content (gutter or overlay).
5. Spinner visible for running subscriptions.
6. Completed subscriptions show summary (N new, N skipped) before lingering and disappearing.

## Test Cases
1. Run a subscription with multi-image posts → verify phases cycle through: downloading → stashing → importing → creating collection → completed.
2. Run a subscription with invalid credentials → verify error shows inline: "Failed: unauthorized".
3. Run 5+ subscriptions simultaneously → verify scrollbar doesn't hide content.
4. Stop a running subscription → verify it shows "Cancelled" before lingering.
