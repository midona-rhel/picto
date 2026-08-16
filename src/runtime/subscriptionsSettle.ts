import { getDefaultStore } from 'jotai';
import { subscriptionsController } from '../controllers/subscriptionsController';
import { listen } from '../platform/ipc';
import {
  subscriptionsCoversAtom,
  subscriptionsSelectionAtom,
  subscriptionsWorkspaceErrorAtom,
  subscriptionsWorkspaceLoadingAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../state/subscriptionsWorkspace';

const authRefreshCallbacks = new Set<() => void>();
const store = getDefaultStore();

const PROGRESS_POLL_MS = 1500;
const POLL_GRACE_MS = 5000;

let workspaceRefreshPromise: Promise<void> | null = null;
let runtimeRefreshPromise: Promise<void> | null = null;
let runGraceUntil = 0;
let syncPolling: (() => void) | null = null;

function trigger(callbacks: Set<() => void>) {
  for (const callback of callbacks) {
    try {
      callback();
    } catch (error) {
      console.error('subscription settle callback failed', error);
    }
  }
}

export function registerAuthWorkspaceRefresh(callback: () => void): () => void {
  authRefreshCallbacks.add(callback);
  return () => {
    authRefreshCallbacks.delete(callback);
  };
}

export function refreshSubscriptionsWorkspace(): Promise<void> {
  if (workspaceRefreshPromise) return workspaceRefreshPromise;

  store.set(subscriptionsWorkspaceErrorAtom, null);
  workspaceRefreshPromise = (async () => {
    try {
      const [snapshot, covers] = await Promise.all([
        subscriptionsController.loadWorkspaceSnapshot(),
        subscriptionsController.getCovers().catch(() => null),
      ]);
      store.set(subscriptionsWorkspaceSnapshotAtom, snapshot);
      if (covers) store.set(subscriptionsCoversAtom, covers);
      store.set(subscriptionsSelectionAtom, (current) => {
        if (current?.kind === 'subscription' && snapshot.subscriptions.some((sub) => sub.id === current.id)) {
          return current;
        }
        return null;
      });
    } catch (error) {
      store.set(subscriptionsWorkspaceErrorAtom, error instanceof Error ? error.message : String(error));
    } finally {
      store.set(subscriptionsWorkspaceLoadingAtom, false);
      workspaceRefreshPromise = null;
    }
  })();
  return workspaceRefreshPromise;
}

export function refreshSubscriptionsRuntimeState(): Promise<void> {
  if (runtimeRefreshPromise) return runtimeRefreshPromise;

  runtimeRefreshPromise = subscriptionsController.refreshRuntimeState()
    .then((runtime) => {
      store.set(subscriptionsWorkspaceSnapshotAtom, (current) => (current ? { ...current, ...runtime } : current));
    })
    .catch(() => {
      // A later event or poll retries transient runtime failures.
    })
    .finally(() => {
      runtimeRefreshPromise = null;
    });
  return runtimeRefreshPromise;
}

export function markSubscriptionRunTriggered(): void {
  runGraceUntil = Date.now() + POLL_GRACE_MS;
  syncPolling?.();
  void refreshSubscriptionsRuntimeState();
}

// Per-item progress events can arrive in bursts; trailing-edge throttle keeps
// refreshes near-instant without hammering the backend.
const PROGRESS_THROTTLE_MS = 150;
let progressThrottleTimer: ReturnType<typeof setTimeout> | null = null;

function triggerProgressThrottled() {
  if (progressThrottleTimer !== null) return;
  progressThrottleTimer = setTimeout(() => {
    progressThrottleTimer = null;
    void refreshSubscriptionsRuntimeState();
  }, PROGRESS_THROTTLE_MS);
}

export function startSubscriptionsSettle(): () => void {
  let cancelled = false;
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  const updatePolling = () => {
    if (cancelled) return;
    const snapshot = store.get(subscriptionsWorkspaceSnapshotAtom);
    const shouldPoll = (snapshot?.runningSubscriptionIds.length ?? 0) > 0 || Date.now() < runGraceUntil;
    if (shouldPoll && pollTimer === null) {
      pollTimer = setInterval(() => {
        void refreshSubscriptionsRuntimeState().finally(updatePolling);
      }, PROGRESS_POLL_MS);
    } else if (!shouldPoll && pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  };
  syncPolling = updatePolling;
  const unsubscribeSnapshot = store.sub(subscriptionsWorkspaceSnapshotAtom, updatePolling);

  const unlistenPromise = listen<{ changes?: { domains?: string[] } }>(
    'runtime/state_changed',
    ({ payload }) => {
      if (cancelled) return;
      const domains = payload.changes?.domains ?? [];
      if (!domains.includes('subscriptions')) return;
      void refreshSubscriptionsWorkspace();
      trigger(authRefreshCallbacks);
    },
  );

  const isSubscriptionTask = (task?: { kind?: string }) => task?.kind === 'subscription';

  const unlistenUpsertPromise = listen<{ task?: { kind?: string } }>(
    'runtime/task_upserted',
    ({ payload }) => {
      if (cancelled || !isSubscriptionTask(payload.task)) return;
      triggerProgressThrottled();
    },
  );
  // task_removed carries only task_id. Subscription tasks use the "sub:<id>" prefix.
  const unlistenRemovePromise = listen<{ task_id?: string }>(
    'runtime/task_removed',
    ({ payload }) => {
      if (cancelled) return;
      const id = payload.task_id ?? '';
      if (!id.startsWith('sub:')) return;
      triggerProgressThrottled();
    },
  );

  return () => {
    cancelled = true;
    unsubscribeSnapshot();
    if (syncPolling === updatePolling) syncPolling = null;
    if (pollTimer !== null) clearInterval(pollTimer);
    if (progressThrottleTimer !== null) {
      clearTimeout(progressThrottleTimer);
      progressThrottleTimer = null;
    }
    for (const p of [unlistenPromise, unlistenUpsertPromise, unlistenRemovePromise]) {
      p.then((fn) => fn()).catch(() => {});
    }
  };
}
