import { create } from 'zustand';
import { listenRuntimeEvent, api, type UnlistenFn } from '#desktop/api';
import type {
  MutationReceipt,
  RuntimeTask,
  RuntimeSnapshot,
  TaskKind,
  SidebarCounts,
  ResourceKey,
} from '../shared/types/generated/runtime-contract';
import { deriveStaleResources } from '../runtime/resourceInvalidator';
import { logBestEffortError } from '../shared/lib/asyncOps';
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

// ---------------------------------------------------------------------------
// Derived types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// State shape
// ---------------------------------------------------------------------------

export interface RuntimeSyncState {
  initialized: boolean;

  // --- Resource invalidation ---
  lastSeq: number;
  tasksById: Map<string, RuntimeTask>;
  staleResources: Set<ResourceKey>;
  sidebarCounts: SidebarCounts | null;
  lastOriginCommand: string | null;

  // --- PTR progress (legacy events) ---
  ptrSyncing: boolean;
  ptrProgress: PtrSyncProgress | null;
  ptrLastResult: PtrSyncResult | null;
  ptrBootstrapStatus: PtrBootstrapStatus | null;

  // --- Subscription progress (legacy events) ---
  runningSubscriptionIds: Set<string>;
  runningQueryIds: Set<string>;
  subscriptionProgressById: Map<string, RuntimeSubscriptionProgress>;
  lastSubscriptionFinished: SubscriptionFinishedEvent | null;
  subscriptionEventSeq: number;

  // --- Flow progress (legacy events) ---
  runningFlowIds: Set<string>;
  flowProgressById: Map<string, FlowProgressEvent>;
  lastFlowFinished: FlowFinishedEvent | null;
  flowEventSeq: number;

  // Actions
  ensureInitialized: () => Promise<void>;
  teardown: () => void;
  applyMutationReceipt: (receipt: MutationReceipt) => void;
  applyTaskUpsert: (task: RuntimeTask) => void;
  applyTaskRemoved: (taskId: string) => void;
  refreshSnapshot: () => Promise<void>;
  refreshTaskSnapshots: () => Promise<void>;
  markResourceFresh: (key: ResourceKey) => void;
  markResourcesStale: (keys: Iterable<ResourceKey>) => void;

  // Selectors
  getTasksByKind: (kind: TaskKind) => RuntimeTask[];
  isAnyTaskRunning: (kind: TaskKind) => boolean;
}

// ---------------------------------------------------------------------------
// Module-level state
// ---------------------------------------------------------------------------

let unlisteners: UnlistenFn[] = [];
let watchdogTimer: ReturnType<typeof setInterval> | null = null;
let lastEventTs = 0;
let isInitializing = false;
const taskLingerTimers = new Map<string, ReturnType<typeof setTimeout>>();
const subFinishedTimers = new Map<string, ReturnType<typeof setTimeout>>();

const WATCHDOG_POLL_MS = 1000;
const WATCHDOG_STALE_MS = 5000;

function clearTimers() {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  for (const timer of taskLingerTimers.values()) clearTimeout(timer);
  taskLingerTimers.clear();
  for (const timer of subFinishedTimers.values()) clearTimeout(timer);
  subFinishedTimers.clear();
}

function lingerMs(task: RuntimeTask): number {
  const detail = task.detail as Record<string, unknown> | undefined;
  const failureKind = detail?.failure_kind;
  if (failureKind === 'inbox_full') return 6000;
  if (task.status === 'failed') return 4500;
  return 2200;
}

function resolveFinishedSubStatusText(event: SubscriptionFinishedEvent): string {
  if (event.status === 'cancelled' && event.failure_kind === 'inbox_full') return 'Paused (Inbox full)';
  if (event.status === 'succeeded') return 'Completed';
  if (event.status === 'cancelled') return 'Cancelled';
  return 'Failed';
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useRuntimeSyncStore = create<RuntimeSyncState>((set, get) => ({
  initialized: false,
  lastSeq: 0,
  tasksById: new Map(),
  staleResources: new Set(),
  sidebarCounts: null,
  lastOriginCommand: null,

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
      // 1. Seed from snapshots
      await Promise.all([
        get().refreshSnapshot(),
        get().refreshTaskSnapshots(),
      ]);

      // 2. Subscribe to all events
      const listeners = await Promise.all([
        // --- Runtime events ---
        listenRuntimeEvent('runtime/mutation_committed', (receipt) => {
          get().applyMutationReceipt(receipt);
        }),
        listenRuntimeEvent('runtime/task_upserted', (task) => {
          get().applyTaskUpsert(task);
        }),
        listenRuntimeEvent('runtime/task_removed', (payload) => {
          get().applyTaskRemoved(payload.task_id);
        }),

        // --- PTR legacy events ---
        PtrSyncController.onStarted(() => {
          set({ ptrSyncing: true, ptrProgress: null, ptrLastResult: null });
        }),
        PtrSyncController.onProgress((progress) => {
          set({ ptrSyncing: true, ptrProgress: progress });
        }),
        PtrSyncController.onFinished((result) => {
          set({ ptrSyncing: false, ptrProgress: null, ptrLastResult: result });
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('runtimeSyncStore.ptrBootstrapStatus.onFinished', error));
        }),
        PtrSyncController.onBootstrapStarted(() => {
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('runtimeSyncStore.ptrBootstrapStatus.onBootstrapStarted', error));
        }),
        PtrSyncController.onBootstrapProgress(() => {
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('runtimeSyncStore.ptrBootstrapStatus.onBootstrapProgress', error));
        }),
        PtrSyncController.onBootstrapFinished(() => {
          void PtrSyncController.getBootstrapStatus()
            .then((status) => set({ ptrBootstrapStatus: status }))
            .catch((error) => logBestEffortError('runtimeSyncStore.ptrBootstrapStatus.onBootstrapFinished', error));
        }),
        PtrSyncController.onBootstrapFailed((payload) => {
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
            .catch((error) => logBestEffortError('runtimeSyncStore.ptrBootstrapStatus.onBootstrapFailed', error));
        }),

        // --- Subscription legacy events ---
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

        // --- Flow legacy events ---
        SubscriptionController.onFlowStarted((event) => {
          set((state) => {
            const runningFlowIds = new Set(state.runningFlowIds);
            runningFlowIds.add(event.flow_id);
            const flowProgressById = new Map(state.flowProgressById);
            flowProgressById.delete(event.flow_id);
            return {
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
              runningFlowIds,
              flowProgressById,
              lastFlowFinished: event,
              flowEventSeq: state.flowEventSeq + 1,
            };
          });
        }),
      ]);
      unlisteners = listeners;

      // 3. Watchdog: poll if idle
      watchdogTimer = setInterval(() => {
        const staleMs = Date.now() - lastEventTs;
        if (staleMs < WATCHDOG_STALE_MS) return;
        void Promise.all([
          get().refreshSnapshot(),
          get().refreshTaskSnapshots(),
        ]);
      }, WATCHDOG_POLL_MS);

      set({ initialized: true });
    } catch (error) {
      logBestEffortError('runtimeSyncStore.ensureInitialized', error);
      for (const fn of unlisteners) fn();
      unlisteners = [];
      clearTimers();
      set({ initialized: false });
    } finally {
      isInitializing = false;
    }
  },

  teardown: () => {
    for (const fn of unlisteners) fn();
    unlisteners = [];
    clearTimers();
    set({
      initialized: false,
      lastSeq: 0,
      tasksById: new Map(),
      staleResources: new Set(),
      sidebarCounts: null,
      lastOriginCommand: null,
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
    });
  },

  applyMutationReceipt: (receipt) => {
    const state = get();
    if (receipt.seq <= state.lastSeq) return;

    lastEventTs = Date.now();
    const newStale = deriveStaleResources(receipt);
    const merged = new Set(state.staleResources);
    for (const key of newStale) merged.add(key);

    set({
      lastSeq: receipt.seq,
      staleResources: merged,
      sidebarCounts: receipt.sidebar_counts ?? state.sidebarCounts,
      lastOriginCommand: receipt.origin_command,
    });
  },

  applyTaskUpsert: (task) => {
    lastEventTs = Date.now();

    set((state) => {
      const tasksById = new Map(state.tasksById);
      tasksById.set(task.task_id, task);
      return { tasksById };
    });

    // Schedule linger removal for finished tasks
    if (task.status === 'finished' || task.status === 'failed') {
      const existing = taskLingerTimers.get(task.task_id);
      if (existing) clearTimeout(existing);

      const timer = setTimeout(() => {
        set((state) => {
          const current = state.tasksById.get(task.task_id);
          if (!current || (current.status !== 'finished' && current.status !== 'failed')) return {};
          const tasksById = new Map(state.tasksById);
          tasksById.delete(task.task_id);
          return { tasksById };
        });
        taskLingerTimers.delete(task.task_id);
      }, lingerMs(task));
      taskLingerTimers.set(task.task_id, timer);
    }
  },

  applyTaskRemoved: (taskId) => {
    lastEventTs = Date.now();
    const timer = taskLingerTimers.get(taskId);
    if (timer) {
      clearTimeout(timer);
      taskLingerTimers.delete(taskId);
    }
    set((state) => {
      const tasksById = new Map(state.tasksById);
      tasksById.delete(taskId);
      return { tasksById };
    });
  },

  refreshSnapshot: async () => {
    try {
      const snapshot: RuntimeSnapshot = await api.runtime.getSnapshot();
      lastEventTs = Date.now();
      set((state) => {
        const tasksById = new Map<string, RuntimeTask>();
        for (const task of snapshot.tasks) {
          tasksById.set(task.task_id, task);
        }
        return {
          lastSeq: Math.max(state.lastSeq, snapshot.seq),
          tasksById,
        };
      });
    } catch (error) {
      logBestEffortError('runtimeSyncStore.refreshSnapshot', error);
    }
  },

  refreshTaskSnapshots: async () => {
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
          logBestEffortError('runtimeSyncStore.runningProgress', error);
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

        // Derive flow state from runtime tasks
        const runningFlowIds = new Set<string>();
        const flowProgressById = new Map<string, FlowProgressEvent>();
        for (const task of state.tasksById.values()) {
          if (task.kind !== 'flow') continue;
          const flowId = task.task_id.replace(/^flow:/, '');
          if (task.status === 'running' || task.status === 'cancelling') {
            runningFlowIds.add(flowId);
            if (task.progress) {
              flowProgressById.set(flowId, {
                flow_id: flowId,
                done: task.progress.done,
                total: task.progress.total,
                remaining: task.progress.total - task.progress.done,
              } as FlowProgressEvent);
            }
          }
        }

        return {
          ptrSyncing,
          ptrProgress: ptrSyncing ? ptrProgress : null,
          ptrBootstrapStatus,
          runningSubscriptionIds,
          runningQueryIds,
          subscriptionProgressById,
          runningFlowIds,
          flowProgressById,
        };
      });
    } catch (error) {
      logBestEffortError('runtimeSyncStore.refreshTaskSnapshots', error);
    }
  },

  markResourceFresh: (key) => {
    set((state) => {
      if (!state.staleResources.has(key)) return {};
      const staleResources = new Set(state.staleResources);
      staleResources.delete(key);
      return { staleResources };
    });
  },

  markResourcesStale: (keys) => {
    set((state) => {
      const staleResources = new Set(state.staleResources);
      for (const key of keys) staleResources.add(key);
      return { staleResources };
    });
  },

  getTasksByKind: (kind) => {
    const tasks: RuntimeTask[] = [];
    for (const task of get().tasksById.values()) {
      if (task.kind === kind) tasks.push(task);
    }
    return tasks;
  },

  isAnyTaskRunning: (kind) => {
    for (const task of get().tasksById.values()) {
      if (task.kind === kind && (task.status === 'running' || task.status === 'cancelling')) {
        return true;
      }
    }
    return false;
  },
}));
