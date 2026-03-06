import { create } from 'zustand';
import type { UnlistenFn } from '#desktop/api';
import {
  PtrSyncController,
  type PtrBootstrapStatus,
  type PtrSyncProgress,
  type PtrSyncResult,
} from '../controllers/ptrSyncController';
import {
  SubscriptionController,
  type FlowFinishedEvent,
  type FlowProgressEvent,
  type SubscriptionFinishedEvent,
  type SubscriptionProgressEvent,
} from '../controllers/subscriptionController';
import { logBestEffortError } from '../lib/asyncOps';

export interface RuntimeSubscriptionProgress {
  subscription_id: string;
  subscription_name: string;
  query_id?: string;
  query_name?: string;
  files_downloaded: number;
  files_skipped: number;
  pages_fetched: number;
  status_text: string;
  status: 'running' | 'finished';
  finished_status?: 'succeeded' | 'failed' | 'cancelled';
  failure_kind?: string | null;
  error?: string | null;
}

interface TaskRuntimeState {
  initialized: boolean;
  lastEventTs: number;

  ptrSyncing: boolean;
  ptrProgress: PtrSyncProgress | null;
  ptrLastResult: PtrSyncResult | null;
  ptrBootstrapStatus: PtrBootstrapStatus | null;

  runningSubscriptionIds: Set<string>;
  runningQueryIds: Set<string>;
  subscriptionProgressById: Map<string, RuntimeSubscriptionProgress>;
  lastSubscriptionFinished: SubscriptionFinishedEvent | null;
  subscriptionEventSeq: number;

  runningFlowIds: Set<string>;
  flowProgressById: Map<string, FlowProgressEvent>;
  lastFlowFinished: FlowFinishedEvent | null;
  flowEventSeq: number;

  ensureInitialized: () => Promise<void>;
  teardown: () => void;
  refreshSnapshots: () => Promise<void>;
}

const WATCHDOG_POLL_MS = 1000;
const WATCHDOG_STALE_MS = 5000;

let unlisteners: UnlistenFn[] = [];
let watchdogTimer: ReturnType<typeof setInterval> | null = null;
let isInitializing = false;
const subFinishedTimers = new Map<string, ReturnType<typeof setTimeout>>();

function clearRuntimeTimers() {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  for (const timer of subFinishedTimers.values()) clearTimeout(timer);
  subFinishedTimers.clear();
}

function resolveFinishedSubStatusText(event: SubscriptionFinishedEvent): string {
  if (event.status === 'cancelled' && event.failure_kind === 'inbox_full') return 'Paused (Inbox full)';
  if (event.status === 'succeeded') return 'Completed';
  if (event.status === 'cancelled') return 'Cancelled';
  return 'Failed';
}

export const useTaskRuntimeStore = create<TaskRuntimeState>((set, get) => ({
  initialized: false,
  lastEventTs: Date.now(),

  ptrSyncing: false,
  ptrProgress: null,
  ptrLastResult: null,
  ptrBootstrapStatus: null,

  runningSubscriptionIds: new Set<string>(),
  runningQueryIds: new Set<string>(),
  subscriptionProgressById: new Map<string, RuntimeSubscriptionProgress>(),
  lastSubscriptionFinished: null,
  subscriptionEventSeq: 0,

  runningFlowIds: new Set<string>(),
  flowProgressById: new Map<string, FlowProgressEvent>(),
  lastFlowFinished: null,
  flowEventSeq: 0,

  ensureInitialized: async () => {
    if (get().initialized || isInitializing) return;
    isInitializing = true;
    try {
      await get().refreshSnapshots();

      const listeners = await Promise.all([
        PtrSyncController.onStarted(() => {
          set((state) => ({
            lastEventTs: Date.now(),
            ptrSyncing: true,
            ptrProgress: null,
            ptrLastResult: null,
            subscriptionEventSeq: state.subscriptionEventSeq,
          }));
        }),
        PtrSyncController.onProgress((progress) => {
          set({
            lastEventTs: Date.now(),
            ptrSyncing: true,
            ptrProgress: progress,
          });
        }),
        PtrSyncController.onFinished((result) => {
          set({
            lastEventTs: Date.now(),
            ptrSyncing: false,
            ptrProgress: null,
            ptrLastResult: result,
          });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('taskRuntimeStore.ptrBootstrapStatus.onFinished', error));
        }),
        PtrSyncController.onBootstrapStarted(() => {
          set({ lastEventTs: Date.now() });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('taskRuntimeStore.ptrBootstrapStatus.onBootstrapStarted', error));
        }),
        PtrSyncController.onBootstrapProgress(() => {
          set({ lastEventTs: Date.now() });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('taskRuntimeStore.ptrBootstrapStatus.onBootstrapProgress', error));
        }),
        PtrSyncController.onBootstrapFinished(() => {
          set({ lastEventTs: Date.now() });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('taskRuntimeStore.ptrBootstrapStatus.onBootstrapFinished', error));
        }),
        PtrSyncController.onBootstrapFailed((payload) => {
          set({ lastEventTs: Date.now() });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => {
              set({
                ptrBootstrapStatus: {
                  ...status,
                  running: false,
                  last_error: payload.error || status.last_error,
                },
              });
            })
            .catch((error) => logBestEffortError('taskRuntimeStore.ptrBootstrapStatus.onBootstrapFailed', error));
        }),

        SubscriptionController.onStarted((event) => {
          const timer = subFinishedTimers.get(event.subscription_id);
          if (timer) {
            clearTimeout(timer);
            subFinishedTimers.delete(event.subscription_id);
          }
          set((state) => {
            const runningSubscriptionIds = new Set(state.runningSubscriptionIds);
            runningSubscriptionIds.add(event.subscription_id);
            const runningQueryIds = new Set(state.runningQueryIds);
            if (event.query_id) runningQueryIds.add(event.query_id);
            const subscriptionProgressById = new Map(state.subscriptionProgressById);
            const existing = subscriptionProgressById.get(event.subscription_id);
            subscriptionProgressById.set(event.subscription_id, {
              subscription_id: event.subscription_id,
              subscription_name:
                (event.subscription_name ?? '').trim()
                || existing?.subscription_name
                || `Subscription ${event.subscription_id}`,
              query_id: event.query_id,
              query_name: event.query_name ?? existing?.query_name,
              files_downloaded: existing?.files_downloaded ?? 0,
              files_skipped: existing?.files_skipped ?? 0,
              pages_fetched: existing?.pages_fetched ?? 0,
              status_text: 'Starting...',
              status: 'running',
            });
            return {
              lastEventTs: Date.now(),
              runningSubscriptionIds,
              runningQueryIds,
              subscriptionProgressById,
              subscriptionEventSeq: state.subscriptionEventSeq + 1,
            };
          });
        }),
        SubscriptionController.onProgress((event: SubscriptionProgressEvent) => {
          set((state) => {
            const runningSubscriptionIds = new Set(state.runningSubscriptionIds);
            runningSubscriptionIds.add(event.subscription_id);
            const runningQueryIds = new Set(state.runningQueryIds);
            if (event.query_id) runningQueryIds.add(event.query_id);

            const subscriptionProgressById = new Map(state.subscriptionProgressById);
            const existing = subscriptionProgressById.get(event.subscription_id);
            subscriptionProgressById.set(event.subscription_id, {
              subscription_id: event.subscription_id,
              subscription_name:
                (event.subscription_name ?? '').trim()
                || existing?.subscription_name
                || `Subscription ${event.subscription_id}`,
              query_id: event.query_id,
              query_name: event.query_name ?? existing?.query_name,
              files_downloaded: event.files_downloaded,
              files_skipped: event.files_skipped,
              pages_fetched: event.pages_fetched,
              status_text: event.status_text,
              status: 'running',
            });
            return {
              lastEventTs: Date.now(),
              runningSubscriptionIds,
              runningQueryIds,
              subscriptionProgressById,
              subscriptionEventSeq: state.subscriptionEventSeq + 1,
            };
          });
        }),
        SubscriptionController.onFinished((event: SubscriptionFinishedEvent) => {
          const lingerMs =
            event.failure_kind === 'inbox_full'
              ? 6000
              : event.status === 'failed'
                ? 4500
                : 2200;
          const existingTimer = subFinishedTimers.get(event.subscription_id);
          if (existingTimer) clearTimeout(existingTimer);

          set((state) => {
            const runningSubscriptionIds = new Set(state.runningSubscriptionIds);
            runningSubscriptionIds.delete(event.subscription_id);
            const runningQueryIds = new Set(state.runningQueryIds);
            if (event.query_id) runningQueryIds.delete(event.query_id);
            const subscriptionProgressById = new Map(state.subscriptionProgressById);
            const existing = subscriptionProgressById.get(event.subscription_id);
            subscriptionProgressById.set(event.subscription_id, {
              subscription_id: event.subscription_id,
              subscription_name:
                (event.subscription_name ?? '').trim()
                || existing?.subscription_name
                || `Subscription ${event.subscription_id}`,
              query_id: event.query_id,
              query_name: event.query_name ?? existing?.query_name,
              files_downloaded: event.files_downloaded,
              files_skipped: event.files_skipped,
              pages_fetched: existing?.pages_fetched ?? 0,
              status_text: resolveFinishedSubStatusText(event),
              status: 'finished',
              finished_status: event.status,
              failure_kind: event.failure_kind,
              error: event.error,
            });
            return {
              lastEventTs: Date.now(),
              runningSubscriptionIds,
              runningQueryIds,
              subscriptionProgressById,
              lastSubscriptionFinished: event,
              subscriptionEventSeq: state.subscriptionEventSeq + 1,
            };
          });

          const timer = setTimeout(() => {
            set((state) => {
              const current = state.subscriptionProgressById.get(event.subscription_id);
              if (!current || current.status !== 'finished') return {};
              const subscriptionProgressById = new Map(state.subscriptionProgressById);
              subscriptionProgressById.delete(event.subscription_id);
              return { subscriptionProgressById };
            });
            subFinishedTimers.delete(event.subscription_id);
          }, lingerMs);
          subFinishedTimers.set(event.subscription_id, timer);
        }),

        SubscriptionController.onFlowStarted((event) => {
          set((state) => {
            const runningFlowIds = new Set(state.runningFlowIds);
            runningFlowIds.add(event.flow_id);
            const flowProgressById = new Map(state.flowProgressById);
            flowProgressById.delete(event.flow_id);
            return {
              lastEventTs: Date.now(),
              runningFlowIds,
              flowProgressById,
              flowEventSeq: state.flowEventSeq + 1,
            };
          });
        }),
        SubscriptionController.onFlowProgress((event) => {
          set((state) => {
            const runningFlowIds = new Set(state.runningFlowIds);
            runningFlowIds.add(event.flow_id);
            const flowProgressById = new Map(state.flowProgressById);
            flowProgressById.set(event.flow_id, event);
            return {
              lastEventTs: Date.now(),
              runningFlowIds,
              flowProgressById,
              flowEventSeq: state.flowEventSeq + 1,
            };
          });
        }),
        SubscriptionController.onFlowFinished((event) => {
          set((state) => {
            const runningFlowIds = new Set(state.runningFlowIds);
            runningFlowIds.delete(event.flow_id);
            const flowProgressById = new Map(state.flowProgressById);
            flowProgressById.delete(event.flow_id);
            return {
              lastEventTs: Date.now(),
              runningFlowIds,
              flowProgressById,
              lastFlowFinished: event,
              flowEventSeq: state.flowEventSeq + 1,
            };
          });
        }),
      ]);
      unlisteners = listeners;

      watchdogTimer = setInterval(() => {
        const staleMs = Date.now() - get().lastEventTs;
        if (staleMs < WATCHDOG_STALE_MS) return;
        void get().refreshSnapshots();
      }, WATCHDOG_POLL_MS);

      set({ initialized: true });
    } catch (error) {
      logBestEffortError('taskRuntimeStore.ensureInitialized', error);
      for (const unlisten of unlisteners) unlisten();
      unlisteners = [];
      clearRuntimeTimers();
      set({ initialized: false });
    } finally {
      isInitializing = false;
    }
  },

  teardown: () => {
    for (const unlisten of unlisteners) unlisten();
    unlisteners = [];
    clearRuntimeTimers();
    set({
      initialized: false,
      ptrSyncing: false,
      ptrProgress: null,
      ptrLastResult: null,
      ptrBootstrapStatus: null,
      runningSubscriptionIds: new Set<string>(),
      runningQueryIds: new Set<string>(),
      subscriptionProgressById: new Map<string, RuntimeSubscriptionProgress>(),
      runningFlowIds: new Set<string>(),
      flowProgressById: new Map<string, FlowProgressEvent>(),
    });
  },

  refreshSnapshots: async () => {
    try {
      const [
        ptrSyncing,
        ptrProgress,
        ptrBootstrapStatus,
        runningSubscriptionIdsRaw,
        runningProgress,
      ] = await Promise.all([
        PtrSyncController.isSyncing(),
        PtrSyncController.getSyncProgress(),
        PtrSyncController.getBootstrapStatus(),
        SubscriptionController.getRunningSubscriptions(),
        SubscriptionController.getRunningSubscriptionProgress().catch((error) => {
          logBestEffortError('taskRuntimeStore.runningProgress', error);
          return [];
        }),
      ]);

      set((state) => {
        const runningSubscriptionIds = new Set<string>([
          ...runningSubscriptionIdsRaw,
          ...runningProgress.map((p) => p.subscription_id),
        ]);
        const runningQueryIds = new Set<string>();
        const subscriptionProgressById = new Map(state.subscriptionProgressById);

        // Remove stale running rows, keep finished rows until linger timers expire.
        for (const [subId, progress] of subscriptionProgressById.entries()) {
          if (progress.status === 'running' && !runningSubscriptionIds.has(subId)) {
            subscriptionProgressById.delete(subId);
          }
        }

        for (const progress of runningProgress) {
          if (progress.query_id) runningQueryIds.add(progress.query_id);
          const existing = subscriptionProgressById.get(progress.subscription_id);
          subscriptionProgressById.set(progress.subscription_id, {
            subscription_id: progress.subscription_id,
            subscription_name:
              (progress.subscription_name ?? '').trim()
              || existing?.subscription_name
              || `Subscription ${progress.subscription_id}`,
            query_id: progress.query_id,
            query_name: progress.query_name ?? existing?.query_name,
            files_downloaded: progress.files_downloaded,
            files_skipped: progress.files_skipped,
            pages_fetched: progress.pages_fetched,
            status_text: progress.status_text,
            status: 'running',
          });
        }

        return {
          lastEventTs: Date.now(),
          ptrSyncing,
          ptrProgress: ptrSyncing ? ptrProgress : null,
          ptrBootstrapStatus,
          runningSubscriptionIds,
          runningQueryIds,
          subscriptionProgressById,
        };
      });
    } catch (error) {
      logBestEffortError('taskRuntimeStore.refreshSnapshots', error);
    }
  },
}));
