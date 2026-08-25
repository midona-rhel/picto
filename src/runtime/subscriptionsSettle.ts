import { getDefaultStore } from 'jotai';
import { subscriptionsController } from '../controllers/subscriptionsController';
import { libraryInvalidation } from './libraryInvalidation';
import {
  subscriptionsCoversAtom,
  subscriptionsSelectionAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../state/subscriptionsWorkspace';
import { showErrorNotification, showSuccessNotification } from '../shared/lib/notifications';
import type { SubscriptionProgressEvent } from '../shared/types/subscriptions';

const authRefreshCallbacks = new Set<() => void>();
const store = getDefaultStore();

const PROGRESS_POLL_MS = 1500;
const POLL_GRACE_MS = 5000;

let workspaceRefreshPromise: Promise<void> | null = null;
let runtimeRefreshPromise: Promise<void> | null = null;
let runGraceUntil = 0;
let syncPolling: (() => void) | null = null;
const observedQueryStatuses = new Map<number, Map<number, string>>();
const notifiedRunIds = new Set<number>();

export function resetSubscriptionsSettleForTests(): void {
  workspaceRefreshPromise = null;
  runtimeRefreshPromise = null;
  runGraceUntil = 0;
  syncPolling = null;
  observedQueryStatuses.clear();
  notifiedRunIds.clear();
}

function isSuccessfulTerminal(status: string): boolean {
  return status === 'completed' || status === 'succeeded' || status === 'success';
}

function completionSummary(posts: number, media: number): string {
  const parts = [`${posts} post${posts === 1 ? '' : 's'} traversed`];
  if (media > 0) parts.push(`${media} media added`);
  return parts.join(' · ');
}

async function observeQueryCompletions(progress: SubscriptionProgressEvent[]): Promise<void> {
  await Promise.all(progress.flatMap((entry) => entry.run_id == null ? [] : [
    subscriptionsController.getRunActivity(entry.run_id).then((activity) => {
      const previous = observedQueryStatuses.get(entry.run_id!);
      const current = new Map(activity.queries.map((query) => [query.query_id, query.status]));
      observedQueryStatuses.set(entry.run_id!, current);
      if (!previous) return;
      for (const query of activity.queries) {
        if (!isSuccessfulTerminal(query.status) || isSuccessfulTerminal(previous.get(query.query_id) ?? '')) continue;
        showSuccessNotification({
          title: 'Query completed',
          message: `${entry.subscription_name} · ${query.query_text} · ${completionSummary(query.counts.posts_traversed, query.counts.ingested)}`,
        });
      }
    }).catch(() => {
      // A later progress poll retries this read.
    }),
  ]));
}

async function observeSubscriptionCompletions(
  previous: SubscriptionProgressEvent[],
  current: SubscriptionProgressEvent[],
): Promise<void> {
  const activeRunIds = new Set(current.flatMap((entry) => entry.run_id == null ? [] : [entry.run_id]));
  await Promise.all(previous.flatMap((entry) => {
    if (entry.run_id == null || activeRunIds.has(entry.run_id) || notifiedRunIds.has(entry.run_id)) return [];
    return [subscriptionsController.getRunActivity(entry.run_id).then((activity) => {
      observedQueryStatuses.delete(entry.run_id!);
      if (!isSuccessfulTerminal(activity.summary.status)) return;
      notifiedRunIds.add(entry.run_id!);
      const completedQueries = activity.queries.filter((query) => isSuccessfulTerminal(query.status)).length;
      showSuccessNotification({
        title: 'Subscription completed',
        message: `${entry.subscription_name} · ${completedQueries} quer${completedQueries === 1 ? 'y' : 'ies'} completed · ${completionSummary(activity.summary.counts.posts_traversed, activity.summary.counts.ingested)}`,
      });
    }).catch(() => {
      // Completion notifications must never affect persisted run settlement.
    })];
  }));
}

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
      const previousSnapshot = store.get(subscriptionsWorkspaceSnapshotAtom);
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
      void observeQueryCompletions(snapshot.runningProgress);
      void observeSubscriptionCompletions(
        previousSnapshot?.runningProgress ?? [],
        snapshot.runningProgress,
      );
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
      const previousProgress = snapshot?.runningProgress ?? [];
      store.set(subscriptionsWorkspaceSnapshotAtom, (current) => (current ? { ...current, ...runtime } : current));
      void observeQueryCompletions(runtime.runningProgress);
      void observeSubscriptionCompletions(previousProgress, runtime.runningProgress);
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
