import { create } from 'zustand';
import { listenRuntimeEvent, api, type UnlistenFn } from '#desktop/api';
import type {
  MutationReceipt,
  RuntimeTask,
  RuntimeSnapshot,
  TaskKind,
  TaskUpsertedEvent,
  TaskRemovedEvent,
  SidebarCounts,
  ResourceKey,
} from '../shared/types/generated/runtime-contract';
import { deriveStaleResources } from '../runtime/resourceInvalidator';
import { logBestEffortError } from '../shared/lib/asyncOps';
import type {
  PtrBootstrapStatus,
  PtrSyncProgress,
  PtrSyncResult,
} from '../controllers/ptrSyncController';
import {
  SubscriptionController,
  type FlowFinishedEvent,
  type FlowProgressEvent,
  type SubscriptionFinishedEvent,
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
        listenRuntimeEvent('runtime/task_upserted', (event: TaskUpsertedEvent) => {
          get().applyTaskUpsert(event.task);
        }),
        listenRuntimeEvent('runtime/task_removed', (event: TaskRemovedEvent) => {
          get().applyTaskRemoved(event.task_id);
        }),

        // All domain state (subscription, flow, PTR) is derived from
        // runtime/task_upserted in applyTaskUpsert.
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
    const isRunning = task.status === 'running' || task.status === 'cancelling';
    const isTerminal = task.status === 'finished' || task.status === 'failed';

    set((state) => {
      const tasksById = new Map(state.tasksById);
      tasksById.set(task.task_id, task);
      const patch: Partial<RuntimeSyncState> = { tasksById };

      // --- Derive flow state from task events ---
      if (task.kind === 'flow') {
        const flowId = task.task_id.replace(/^flow:/, '');
        const runningFlowIds = new Set(state.runningFlowIds);
        const flowProgressById = new Map(state.flowProgressById);
        if (isRunning) {
          runningFlowIds.add(flowId);
          if (task.progress) {
            flowProgressById.set(flowId, {
              flow_id: flowId,
              done: task.progress.done,
              total: task.progress.total,
              remaining: task.progress.total - task.progress.done,
            } as FlowProgressEvent);
          }
        } else if (isTerminal) {
          runningFlowIds.delete(flowId);
          flowProgressById.delete(flowId);
          patch.lastFlowFinished = {
            flow_id: flowId,
            status: task.status === 'finished' ? 'succeeded' : 'failed',
          } as FlowFinishedEvent;
        }
        patch.runningFlowIds = runningFlowIds;
        patch.flowProgressById = flowProgressById;
        patch.flowEventSeq = state.flowEventSeq + 1;
      }

      // --- Derive subscription state from task events ---
      if (task.kind === 'subscription') {
        const detail = task.detail as Record<string, unknown> | undefined;
        if (detail) {
          const subId = (detail.subscription_id as string) ?? task.task_id.replace(/^sub:/, '');
          const timer = subFinishedTimers.get(subId);
          if (timer && isRunning) {
            clearTimeout(timer);
            subFinishedTimers.delete(subId);
          }
          const runningSubscriptionIds = new Set(state.runningSubscriptionIds);
          const runningQueryIds = new Set(state.runningQueryIds);
          const subscriptionProgressById = new Map(state.subscriptionProgressById);
          const existing = subscriptionProgressById.get(subId);

          if (isRunning) {
            runningSubscriptionIds.add(subId);
            if (detail.query_id) runningQueryIds.add(detail.query_id as string);
            subscriptionProgressById.set(subId, {
              subscription_id: subId,
              subscription_name:
                ((detail.subscription_name as string) ?? '').trim()
                || existing?.subscription_name
                || `Subscription ${subId}`,
              query_id: detail.query_id as string | undefined,
              query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
              files_downloaded: (detail.files_downloaded as number) ?? 0,
              files_skipped: (detail.files_skipped as number) ?? 0,
              pages_fetched: (detail.pages_fetched as number) ?? 0,
              status_text: (detail.status_text as string) ?? 'Running...',
              status: 'running',
            });
          } else if (isTerminal) {
            runningSubscriptionIds.delete(subId);
            if (detail.query_id) runningQueryIds.delete(detail.query_id as string);
            const finishedStatus =
              (detail.finished_status as string)
              ?? (task.status === 'finished' ? 'succeeded' : 'failed');
            subscriptionProgressById.set(subId, {
              subscription_id: subId,
              subscription_name:
                ((detail.subscription_name as string) ?? '').trim()
                || existing?.subscription_name
                || `Subscription ${subId}`,
              query_id: detail.query_id as string | undefined,
              query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
              files_downloaded: (detail.files_downloaded as number) ?? 0,
              files_skipped: (detail.files_skipped as number) ?? 0,
              pages_fetched: (detail.pages_fetched as number) ?? existing?.pages_fetched ?? 0,
              status_text: (detail.status_text as string) ?? resolveFinishedSubStatusText({
                status: finishedStatus as 'succeeded' | 'failed' | 'cancelled',
                failure_kind: detail.failure_kind as string | undefined,
              } as SubscriptionFinishedEvent),
              status: 'finished',
              finished_status: finishedStatus as 'succeeded' | 'failed' | 'cancelled',
              failure_kind: detail.failure_kind as string | undefined,
              error: detail.error as string | undefined,
            });
            patch.lastSubscriptionFinished = {
              subscription_id: subId,
              subscription_name: (detail.subscription_name as string) ?? '',
              status: finishedStatus,
              files_downloaded: (detail.files_downloaded as number) ?? 0,
              files_skipped: (detail.files_skipped as number) ?? 0,
              failure_kind: detail.failure_kind as string | undefined,
              error: detail.error as string | undefined,
            } as SubscriptionFinishedEvent;
          }
          patch.runningSubscriptionIds = runningSubscriptionIds;
          patch.runningQueryIds = runningQueryIds;
          patch.subscriptionProgressById = subscriptionProgressById;
          patch.subscriptionEventSeq = state.subscriptionEventSeq + 1;
        }
      }

      // --- Derive PTR sync state from task events ---
      if (task.kind === 'ptr_sync') {
        patch.ptrSyncing = isRunning;
        const detail = task.detail as Record<string, unknown> | undefined;
        if (isRunning && detail) {
          // Running detail is PtrSyncProgress
          patch.ptrProgress = detail as unknown as PtrSyncProgress;
        } else if (isTerminal && detail) {
          // Terminal detail is PtrSyncResult
          patch.ptrProgress = null;
          patch.ptrLastResult = detail as unknown as PtrSyncResult;
        } else if (!isRunning) {
          patch.ptrProgress = null;
        }
      }

      // --- Derive PTR bootstrap state from task events ---
      if (task.kind === 'ptr_bootstrap') {
        const detail = task.detail as unknown as PtrBootstrapStatus | undefined;
        if (detail) {
          patch.ptrBootstrapStatus = detail;
        }
      }

      return patch;
    });

    // Schedule linger removal for finished/failed tasks
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

      // Schedule subscription progress cleanup (linger then remove)
      if (task.kind === 'subscription') {
        const detail = task.detail as Record<string, unknown> | undefined;
        const subId = (detail?.subscription_id as string) ?? task.task_id.replace(/^sub:/, '');
        const existingSub = subFinishedTimers.get(subId);
        if (existingSub) clearTimeout(existingSub);
        const subTimer = setTimeout(() => {
          set((state) => {
            const current = state.subscriptionProgressById.get(subId);
            if (!current || current.status !== 'finished') return {};
            const subscriptionProgressById = new Map(state.subscriptionProgressById);
            subscriptionProgressById.delete(subId);
            return { subscriptionProgressById };
          });
          subFinishedTimers.delete(subId);
        }, lingerMs(task));
        subFinishedTimers.set(subId, subTimer);
      }
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
        runningSubscriptionIdsRaw,
        runningProgress,
      ] = await Promise.all([
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
        // Derive PTR state from runtime tasks
        let ptrSyncing = false;
        let ptrProgress: PtrSyncProgress | null = null;
        let ptrBootstrapStatus: PtrBootstrapStatus | null = state.ptrBootstrapStatus;

        for (const task of state.tasksById.values()) {
          if (task.kind === 'flow') {
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
          } else if (task.kind === 'ptr_sync') {
            if (task.status === 'running' || task.status === 'cancelling') {
              ptrSyncing = true;
              if (task.detail) ptrProgress = task.detail as unknown as PtrSyncProgress;
            }
          } else if (task.kind === 'ptr_bootstrap') {
            if (task.detail) ptrBootstrapStatus = task.detail as unknown as PtrBootstrapStatus;
          }
        }

        return {
          ptrSyncing,
          ptrProgress,
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
