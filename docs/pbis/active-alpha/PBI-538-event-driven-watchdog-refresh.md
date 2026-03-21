# PBI-538: Event-driven watchdog refresh

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-21). The polling concern is based on the 1-second `setInterval` in `runtimeSyncStore.ts`. For a desktop app, 1-second polling is not expensive. The concern is about unnecessary timer activity when the library is idle and about the 30-second stale threshold potentially being too aggressive or too lenient depending on context.

## Priority
P3

## Audit Status (2026-03-21)
Status: **Not Implemented**

Evidence: `src/state/runtimeSyncStore.ts` line 60-61 defines a watchdog that polls every 1 second (`WATCHDOG_POLL_MS = 1000`). If no events have been received for 30 seconds (`WATCHDOG_STALE_MS = 30000`), it triggers a force refresh by calling `refreshSnapshot()`. This runs continuously — even when the user is idle (no writes, no UI interaction, library fully quiescent).

## Problem
The runtimeSyncStore watchdog polls every 1 second and force-refreshes after 30 seconds of silence. In a quiescent library (user is browsing, no writes), this creates unnecessary timer activity:

1. **1-second polling**: The interval fires continuously, checking `lastEventTs` against `Date.now()`. This is cheap but unnecessary when no events are expected.
2. **30-second force refresh**: After 30 seconds of no events, `refreshSnapshot()` re-fetches the entire runtime snapshot from the backend. In a quiet library, this is pure waste — the snapshot hasn't changed.
3. **No window focus awareness**: The watchdog runs even when the app is backgrounded. Electron windows in the background don't need real-time state.

## Scope
- `src/state/runtimeSyncStore.ts` — watchdog timer logic
- `src/app/useAppBootstrap.ts` — app-level event listeners (for focus/blur)

## Implementation
1. **Pause watchdog when idle**: After the 30-second stale threshold fires once with no change, switch to a longer interval (e.g., 60 seconds) until the next event arrives.
2. **Pause on window blur**: When the Electron window loses focus, pause the watchdog entirely. Resume on focus with an immediate `refreshSnapshot()` to catch up.
3. **Event-driven reset**: When a runtime event arrives, reset the stale timer and restore the 1-second interval (if it was degraded).
4. **Electron integration**: Use `window.addEventListener('focus', ...)` and `window.addEventListener('blur', ...)` in the renderer to detect visibility changes.

## Acceptance Criteria
1. Watchdog does not fire `refreshSnapshot()` when no backend changes have occurred.
2. Backgrounded windows do not run the watchdog timer.
3. Foregrounding a window triggers an immediate snapshot refresh.
4. Active libraries (events flowing) behave exactly as before.

## Test Cases
1. Open library → wait 60 seconds with no interaction → verify `refreshSnapshot()` called at most once (not every 30 seconds).
2. Background the window → wait 60 seconds → verify zero watchdog polls.
3. Background → make a change via another window → foreground → verify immediate refresh catches the change.
4. Active import (events flowing) → watchdog resets normally every 1 second.

## Risk
Low. The watchdog is a safety net for dropped events — reducing its frequency when idle doesn't affect correctness. The focus/blur optimization is the highest-value change. The main risk is if `refreshSnapshot()` is relied upon for some periodic side-effect beyond state recovery — review the function body before changing the timer.
