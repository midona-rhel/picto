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

  // Actions
  ensureInitialized: () => Promise<void>;
  teardown: () => void;
  applyMutationReceipt: (receipt: MutationReceipt) => void;
  applyTaskUpsert: (task: RuntimeTask) => void;
  applyTaskRemoved: (taskId: string) => void;
  refreshSnapshot: () => Promise<void>;
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

// Mutation receipt batching — coalesce rapid-fire events (e.g. bulk import)
// into a single store update to avoid redundant deriveStaleResources calls.
let pendingReceipts: MutationReceipt[] = [];
let receiptFlushTimer: ReturnType<typeof setTimeout> | null = null;

const WATCHDOG_POLL_MS = 1000;
const WATCHDOG_STALE_MS = 30000;

function clearTimers() {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  if (receiptFlushTimer) {
    clearTimeout(receiptFlushTimer);
    receiptFlushTimer = null;
  }
  pendingReceipts.length = 0;
  for (const timer of taskLingerTimers.values()) clearTimeout(timer);
  taskLingerTimers.clear();
}

function lingerMs(task: RuntimeTask): number {
  const detail = task.detail as Record<string, unknown> | undefined;
  const failureKind = detail?.failure_kind;
  if (failureKind === 'inbox_full') return 6000;
  if (task.status === 'failed') return 4500;
  return 2200;
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

  ensureInitialized: async () => {
    if (get().initialized || isInitializing) return;
    isInitializing = true;
    try {
      // 1. Seed from snapshot
      await get().refreshSnapshot();

      // 2. Subscribe to runtime events
      const listeners = await Promise.all([
        listenRuntimeEvent('runtime/mutation_committed', (receipt) => {
          get().applyMutationReceipt(receipt);
        }),
        listenRuntimeEvent('runtime/task_upserted', (event: TaskUpsertedEvent) => {
          get().applyTaskUpsert(event.task);
        }),
        listenRuntimeEvent('runtime/task_removed', (event: TaskRemovedEvent) => {
          get().applyTaskRemoved(event.task_id);
        }),
      ]);
      unlisteners = listeners;

      // 3. Watchdog: poll if idle
      watchdogTimer = setInterval(() => {
        const staleMs = Date.now() - lastEventTs;
        if (staleMs < WATCHDOG_STALE_MS) return;
        void get().refreshSnapshot();
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
    });
  },

  applyMutationReceipt: (receipt) => {
    lastEventTs = Date.now();
    pendingReceipts.push(receipt);
    if (!receiptFlushTimer) {
      receiptFlushTimer = setTimeout(() => {
        receiptFlushTimer = null;
        const batch = pendingReceipts.splice(0);
        if (batch.length === 0) return;
        const state = get();
        const merged = new Set(state.staleResources);
        let maxSeq = state.lastSeq;
        let latestSidebarCounts = state.sidebarCounts;
        let latestOriginCommand = state.lastOriginCommand;
        for (const r of batch) {
          if (r.seq <= maxSeq) continue;
          maxSeq = r.seq;
          const newStale = deriveStaleResources(r);
          for (const key of newStale) merged.add(key);
          if (r.sidebar_counts) latestSidebarCounts = r.sidebar_counts;
          latestOriginCommand = r.origin_command;
        }
        if (maxSeq <= state.lastSeq) return;
        set({
          lastSeq: maxSeq,
          staleResources: merged,
          sidebarCounts: latestSidebarCounts,
          lastOriginCommand: latestOriginCommand,
        });
      }, 50);
    }
  },

  applyTaskUpsert: (task) => {
    lastEventTs = Date.now();

    set((state) => {
      const tasksById = new Map(state.tasksById);
      tasksById.set(task.task_id, task);
      return { tasksById };
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
