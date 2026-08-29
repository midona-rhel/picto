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
import type { SubscriptionWorkspaceSnapshot } from '../shared/types/subscriptionsWorkspace';
import { isGalleryImportJob } from '../features/subscriptions/subscriptionUtils';

const authRefreshCallbacks = new Set<() => void>();
const store = getDefaultStore();

const PROGRESS_POLL_MS = 1500;
const POLL_GRACE_MS = 5000;
const WORKSPACE_INVALIDATION_DEBOUNCE_MS = 250;
const RUN_ACTIVITY_REFRESH_MS = 10_000;

let workspaceRefreshPromise: Promise<void> | null = null;
let workspaceRefreshQueued = false;
let runtimeRefreshPromise: Promise<void> | null = null;
let runGraceUntil = 0;
let syncPolling: (() => void) | null = null;
const observedQueryStatuses = new Map<number, Map<number, string>>();
const observedRunProgress = new Map<number, { fingerprint: string; checkedAt: number }>();
const runActivityReads = new Map<number, Promise<Awaited<ReturnType<typeof subscriptionsController.getRunActivity>>>>();
const notifiedRunIds = new Set<number>();
const settlingGalleryIds = new Set<string>();

export function retainGalleryProgressTotals(
  previous: SubscriptionProgressEvent[],
  current: SubscriptionProgressEvent[],
): SubscriptionProgressEvent[] {
  const knownByRun = new Map(previous.flatMap((entry) => (
    entry.run_id != null && entry.gallery_total_items != null
      ? [[`${entry.subscription_id}:${entry.run_id}`, entry.gallery_total_items] as const]
      : []
  )));
  return current.map((entry) => {
    if (entry.run_id == null) return entry;
    const known = knownByRun.get(`${entry.subscription_id}:${entry.run_id}`) ?? null;
    const reported = entry.gallery_total_items ?? null;
    const total = known == null ? reported : reported == null ? known : Math.max(known, reported);
    return total === reported ? entry : { ...entry, gallery_total_items: total };
  });
}

export function resetSubscriptionsSettleForTests(): void {
  workspaceRefreshPromise = null;
  workspaceRefreshQueued = false;
  runtimeRefreshPromise = null;
  runGraceUntil = 0;
  syncPolling = null;
  observedQueryStatuses.clear();
  observedRunProgress.clear();
  runActivityReads.clear();
  notifiedRunIds.clear();
  settlingGalleryIds.clear();
}

function isSuccessfulTerminal(status: string): boolean {
  return status === 'completed' || status === 'succeeded' || status === 'success';
}

function completionSummary(postsAdded: number): string {
  return `${postsAdded} post${postsAdded === 1 ? '' : 's'} added to library`;
}

function runProgressFingerprint(entry: SubscriptionProgressEvent): string {
  return [
    entry.query_id ?? '',
    entry.phase ?? '',
    entry.finished_status ?? '',
    entry.failure_kind ?? '',
  ].join('\u0000');
}

function getRunActivityCoalesced(runId: number) {
  const pending = runActivityReads.get(runId);
  if (pending) return pending;
  const read = subscriptionsController.getRunActivity(runId).finally(() => {
    if (runActivityReads.get(runId) === read) runActivityReads.delete(runId);
  });
  runActivityReads.set(runId, read);
  return read;
}

async function observeQueryCompletions(
  progress: SubscriptionProgressEvent[],
  ignoredSubscriptionIds = new Set<string>(),
): Promise<void> {
  await Promise.all(progress.flatMap((entry) => {
    if (entry.run_id == null || ignoredSubscriptionIds.has(entry.subscription_id)) return [];
    const runId = entry.run_id;
    const fingerprint = runProgressFingerprint(entry);
    const previousObservation = observedRunProgress.get(runId);
    const checkedAt = Date.now();
    if (previousObservation?.fingerprint === fingerprint
      && checkedAt - previousObservation.checkedAt < RUN_ACTIVITY_REFRESH_MS) return [];
    // Record before starting the read so overlapping workspace/runtime refreshes
    // cannot fan out into duplicate activity requests for the same run state.
    observedRunProgress.set(runId, { fingerprint, checkedAt });
    return [getRunActivityCoalesced(runId).then((activity) => {
      const previous = observedQueryStatuses.get(entry.run_id!);
      const current = new Map(activity.queries.map((query) => [query.query_id, query.status]));
      observedQueryStatuses.set(entry.run_id!, current);
      if (!previous) return;
      for (const query of activity.queries) {
        if (!isSuccessfulTerminal(query.status) || isSuccessfulTerminal(previous.get(query.query_id) ?? '')) continue;
        showSuccessNotification({
          title: 'Query completed',
          message: `${entry.subscription_name} · ${query.query_text} · ${completionSummary(query.counts.posts_added)}`,
        });
      }
    }).catch(() => {
      // Retry unchanged progress after a transient read failure.
      if (observedRunProgress.get(runId)?.checkedAt === checkedAt) observedRunProgress.delete(runId);
    })];
  }));
}

async function observeSubscriptionCompletions(
  previous: SubscriptionProgressEvent[],
  current: SubscriptionProgressEvent[],
  ignoredSubscriptionIds = new Set<string>(),
): Promise<void> {
  const activeRunIds = new Set(current.flatMap((entry) => entry.run_id == null ? [] : [entry.run_id]));
  await Promise.all(previous.flatMap((entry) => {
    if (entry.run_id == null
      || ignoredSubscriptionIds.has(entry.subscription_id)
      || activeRunIds.has(entry.run_id)
      || notifiedRunIds.has(entry.run_id)) return [];
    return [getRunActivityCoalesced(entry.run_id).then((activity) => {
      observedQueryStatuses.delete(entry.run_id!);
      observedRunProgress.delete(entry.run_id!);
      if (!isSuccessfulTerminal(activity.summary.status)) return;
      notifiedRunIds.add(entry.run_id!);
      const completedQueries = activity.queries.filter((query) => isSuccessfulTerminal(query.status)).length;
      showSuccessNotification({
        title: 'Subscription completed',
        message: `${entry.subscription_name} · ${completedQueries} quer${completedQueries === 1 ? 'y' : 'ies'} completed · ${completionSummary(activity.summary.counts.posts_added)}`,
      });
    }).catch(() => {
      // Completion notifications must never affect persisted run settlement.
    })];
  }));
}

function settleFinishedGalleryImports(snapshot: SubscriptionWorkspaceSnapshot): void {
  const runningIds = new Set(snapshot.runningSubscriptionIds);
  for (const job of snapshot.subscriptions.filter(isGalleryImportJob)) {
    if (runningIds.has(job.id) || settlingGalleryIds.has(job.id)) continue;
    settlingGalleryIds.add(job.id);
    void subscriptionsController.listRuns(job.id).then(async (runs) => {
      const latest = runs[0];
      // Creation and run startup are separate native operations. An invalidation can
      // expose the queued job before its run record exists; leave it alone and retry.
      if (!latest) return;
      if (latest.status === 'succeeded') {
        const cleanup = await subscriptionsController.cleanupGalleryImport(job.id);
        showSuccessNotification({
          title: 'Gallery downloaded',
          message: `${cleanup?.title ?? job.name} has been downloaded.`,
        });
      } else {
        showErrorNotification({
          title: 'Gallery import failed',
          message: latest.error_message ?? `Gallery worker failed (${latest.failure_kind ?? latest.status}).`,
        });
        await subscriptionsController.cleanupGalleryImport(job.id);
      }
      store.set(subscriptionsWorkspaceSnapshotAtom, (current) => current ? {
        ...current,
        subscriptions: current.subscriptions.filter((subscription) => subscription.id !== job.id),
        runningSubscriptionIds: current.runningSubscriptionIds.filter((id) => id !== job.id),
        runningProgress: current.runningProgress.filter((entry) => entry.subscription_id !== job.id),
      } : current);
    }).catch((error) => {
      console.error('gallery import cleanup failed', error);
    }).finally(() => {
      settlingGalleryIds.delete(job.id);
    });
  }
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
  if (workspaceRefreshPromise) {
    workspaceRefreshQueued = true;
    return workspaceRefreshPromise;
  }

  workspaceRefreshPromise = (async () => {
    do {
      workspaceRefreshQueued = false;
      try {
        const previousSnapshot = store.get(subscriptionsWorkspaceSnapshotAtom);
        const loaded = await subscriptionsController.loadWorkspaceSnapshot();
        const snapshot = {
          ...loaded,
          runningProgress: retainGalleryProgressTotals(
            previousSnapshot?.runningProgress ?? [],
            loaded.runningProgress,
          ),
        };
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
          if (current?.kind === 'subscription' && snapshot.subscriptions.some(
            (sub) => sub.id === current.id && !isGalleryImportJob(sub),
          )) {
            return current;
          }
          return null;
        });
        const galleryIds = new Set(snapshot.subscriptions.filter(isGalleryImportJob).map((job) => job.id));
        void observeQueryCompletions(snapshot.runningProgress, galleryIds);
        void observeSubscriptionCompletions(
          previousSnapshot?.runningProgress ?? [],
          snapshot.runningProgress,
          galleryIds,
        );
        settleFinishedGalleryImports(snapshot);
      } catch (error) {
        showErrorNotification({
          title: 'Subscriptions unavailable',
          message: error instanceof Error ? error.message : String(error),
        });
      }
    } while (workspaceRefreshQueued);
  })();
  void workspaceRefreshPromise.finally(() => {
    workspaceRefreshPromise = null;
    syncPolling?.();
  });
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
      const settledRuntime = {
        ...runtime,
        runningProgress: retainGalleryProgressTotals(previousProgress, runtime.runningProgress),
      };
      store.set(subscriptionsWorkspaceSnapshotAtom, (current) => (
        current ? { ...current, ...settledRuntime } : current
      ));
      const galleryIds = new Set(snapshot?.subscriptions.filter(isGalleryImportJob).map((job) => job.id) ?? []);
      void observeQueryCompletions(settledRuntime.runningProgress, galleryIds);
      void observeSubscriptionCompletions(previousProgress, settledRuntime.runningProgress, galleryIds);
      if (snapshot) settleFinishedGalleryImports({ ...snapshot, ...settledRuntime });
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
  let workspaceInvalidationTimer: ReturnType<typeof setTimeout> | null = null;

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
    if (workspaceInvalidationTimer !== null) clearTimeout(workspaceInvalidationTimer);
    workspaceInvalidationTimer = setTimeout(() => {
      workspaceInvalidationTimer = null;
      if (cancelled) return;
      void refreshSubscriptionsWorkspace();
      trigger(authRefreshCallbacks);
    }, WORKSPACE_INVALIDATION_DEBOUNCE_MS);
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
    if (workspaceInvalidationTimer !== null) clearTimeout(workspaceInvalidationTimer);
  };
}
