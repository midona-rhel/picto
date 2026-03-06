# PBI-259: Subscription import does not refresh inbox sidebar count

## Priority
P1

## Audit Status (2026-03-06)
Status: **Blocked (Subscription Workstream Deferred)**

Blocked Reason:
1. Subscription workstream is deferred by product direction for now.
2. Keep this PBI in backlog but do not execute until unblocked.

Evidence:
1. During active subscription downloads, new files are imported into the library but the inbox count shown in the sidebar does not increment live.
2. The backend can continue importing successfully while the sidebar remains stale, so the issue is count invalidation/event propagation rather than import failure.
3. The stale count makes the app look idle or incorrect during subscription ingestion.

## Problem
Subscription-driven imports do not refresh the inbox badge/count in the sidebar as files land. Users can have new inbox items present in the library while the sidebar still shows the old count until some unrelated refresh path happens.

## Scope
- `core/src/subscription_sync.rs`
- `core/src/events.rs`
- `core/src/sidebar_controller.rs`
- `src/stores/eventBridge.ts`
- `src/stores/domainStore.ts`
- `src/components/sidebar/Sidebar.tsx`

## Implementation
1. Audit the subscription import success path and duplicate/restore paths to confirm which mutation impacts are emitted for:
   - new file imported to inbox
   - trashed file restored by subscription import
   - duplicate auto-merge where inbox membership changes
2. Ensure those paths emit a sidebar count invalidation or equivalent domain refresh signal every time inbox membership changes.
3. Verify the frontend event bridge consumes that signal and updates sidebar counts without requiring route change, manual refresh, or app restart.
4. Add a regression test covering live subscription imports updating the inbox count while the run is still active.

## Acceptance Criteria
1. When a subscription imports a new inbox item, the sidebar inbox count increments during the run.
2. When a subscription restores a trashed file into inbox, the sidebar count updates immediately.
3. Auto-merged duplicates do not leave the sidebar count in a stale state.
4. No manual refresh, route switch, or app restart is required to see the correct inbox count.

## Test Cases
1. Start a subscription that imports 10 brand-new files; the inbox count increases as those files land.
2. Start a subscription that restores previously trashed files; the inbox count updates immediately for restored items.
3. Start a subscription where some files auto-merge as duplicates; the final inbox count matches actual inbox membership.
4. Keep the sidebar visible during the run; count changes appear without navigating away.

## Risk
Medium. This crosses backend mutation impact emission, runtime event bridging, and sidebar state refresh behavior.
