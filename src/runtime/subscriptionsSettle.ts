import { getDefaultStore } from 'jotai';
import { subscriptionsController } from '../controllers/subscriptionsController';
import { libraryInvalidation } from './libraryInvalidation';
import {
  subscriptionsCoversAtom,
  subscriptionsSelectionAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../state/subscriptionsWorkspace';
import { showErrorNotification } from '../shared/lib/notifications';

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

  workspaceRefreshPromise = (async () => {
    try {
      const snapshot = await subscriptionsController.loadWorkspaceSnapshot();
      const covers = snapshot.covers;
      const previousCovers = store.get(subscriptionsCoversAtom);
      for (const subscriptionId of snapshot.runningSubscriptionIds) {
        const previousCover = previousCovers.get(subscriptionId);
        if (previousCover) covers.set(subscriptionId, previousCover);
        else covers.delete(subscriptionId);
      }
      store.set(subscriptionsWorkspaceSnapshotAtom, snapshot);
      store.set(subscriptionsCoversAtom, covers);
      store.set(subscriptionsSelectionAtom, (current) => {
        if (current?.kind === 'subscription' && snapshot.subscriptions.some((sub) => sub.id === current.id)) {
          return current;
        }
        return null;
      });
    } catch (error) {
      showErrorNotification({
        title: 'Subscriptions unavailable',
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      workspaceRefreshPromise = null;
      syncPolling?.();
    }
  })();
  return workspaceRefreshPromise;
}

export function refreshSubscriptionsRuntimeState(): Promise<void> {
  if (runtimeRefreshPromise) return runtimeRefreshPromise;

  const snapshot = store.get(subscriptionsWorkspaceSnapshotAtom);
  const runningIds = new Set(snapshot?.runningSubscriptionIds ?? []);
  const runningSubscriptions = snapshot?.subscriptions.filter((subscription) => runningIds.has(subscription.id)) ?? [];
  runtimeRefreshPromise = subscriptionsController.refreshRuntimeState(runningSubscriptions)
    .then((runtime) => {
      store.set(subscriptionsWorkspaceSnapshotAtom, (current) => (current ? { ...current, ...runtime } : current));
    })
    .catch(() => {
      // A later invalidation or poll retries transient runtime failures.
    })
    .finally(() => {
      runtimeRefreshPromise = null;
      syncPolling?.();
    });
  return runtimeRefreshPromise;
}

export function markSubscriptionRunTriggered(): void {
  runGraceUntil = Date.now() + POLL_GRACE_MS;
  syncPolling?.();
  void refreshSubscriptionsRuntimeState();
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
        void refreshSubscriptionsRuntimeState();
      }, PROGRESS_POLL_MS);
    } else if (!shouldPoll && pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  };

  syncPolling = updatePolling;
  const unsubscribeSnapshot = store.sub(subscriptionsWorkspaceSnapshotAtom, updatePolling);
  const unregisterSubscriptions = libraryInvalidation.register('subscriptions', () => {
    if (cancelled) return;
    void refreshSubscriptionsWorkspace();
    trigger(authRefreshCallbacks);
  });
  const unregisterTasks = libraryInvalidation.register('tasks', () => {
    if (cancelled) return;
    void refreshSubscriptionsRuntimeState();
  });

  updatePolling();

  return () => {
    cancelled = true;
    unsubscribeSnapshot();
    unregisterSubscriptions();
    unregisterTasks();
    if (syncPolling === updatePolling) syncPolling = null;
    if (pollTimer !== null) clearInterval(pollTimer);
  };
}
