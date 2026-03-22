import { create } from 'zustand';
import { listenRuntimeEvent, getRuntimeSnapshot, type UnlistenFn } from '#desktop/api';
import type {
  StateChangedEvent,
  RuntimeTask,
  RuntimeSnapshot,
  TaskKind,
  TaskUpsertedEvent,
  TaskRemovedEvent,
  SidebarCounts,
  ResourceKey,
} from '../../shared/types/backendState';
import { planRefreshTargets } from './planRefreshTargets';
import { logBestEffortError } from '../../shared/lib/asyncOps';

// ---------------------------------------------------------------------------
// State shape
// ---------------------------------------------------------------------------

export interface StateChangeStoreState {
  initialized: boolean;

  // --- Refresh targets waiting to be applied ---
  lastSeq: number;
  tasksById: Map<string, RuntimeTask>;
  pendingRefreshTargets: Set<ResourceKey>;
  lastPlannedRefreshTargets: Set<ResourceKey>;
  refreshTargetVersion: number;
  sidebarCounts: SidebarCounts | null;
  lastChangeOrigin: string | null;

  // Actions
  ensureInitialized: () => Promise<void>;
  teardown: () => void;
  applyStateChangedEvent: (event: StateChangedEvent) => void;
  applyTaskUpsert: (task: RuntimeTask) => void;
  applyTaskRemoved: (taskId: string) => void;
  refreshSnapshot: () => Promise<void>;
  markRefreshTargetHandled: (key: ResourceKey) => void;
  queueRefreshTargets: (keys: Iterable<ResourceKey>) => void;

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

// State-changed batching — coalesce rapid-fire events (e.g. bulk import)
// into a single store update to avoid redundant refresh-target planning.
let pendingStateChanges: StateChangedEvent[] = [];
let stateChangeFlushTimer: ReturnType<typeof setTimeout> | null = null;

const WATCHDOG_POLL_MS = 1000;
const WATCHDOG_STALE_MS = 30000;

function clearTimers() {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  if (stateChangeFlushTimer) {
    clearTimeout(stateChangeFlushTimer);
    stateChangeFlushTimer = null;
  }
  pendingStateChanges.length = 0;
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

export const useStateChangeStore = create<StateChangeStoreState>((set, get) => ({
  initialized: false,
  lastSeq: 0,
  tasksById: new Map(),
  pendingRefreshTargets: new Set(),
  lastPlannedRefreshTargets: new Set(),
  refreshTargetVersion: 0,
  sidebarCounts: null,
  lastChangeOrigin: null,

  ensureInitialized: async () => {
    if (get().initialized || isInitializing) return;
    isInitializing = true;
    try {
      // 1. Seed from snapshot
      await get().refreshSnapshot();

      // 2. Subscribe to runtime events
      const listeners = await Promise.all([
        listenRuntimeEvent('runtime/state_changed', (event) => {
          get().applyStateChangedEvent(event);
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
      logBestEffortError('stateChangeStore.ensureInitialized', error);
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
      pendingRefreshTargets: new Set(),
      lastPlannedRefreshTargets: new Set(),
      refreshTargetVersion: 0,
      sidebarCounts: null,
      lastChangeOrigin: null,
    });
  },

  applyStateChangedEvent: (event) => {
    lastEventTs = Date.now();
    pendingStateChanges.push(event);
    if (!stateChangeFlushTimer) {
      stateChangeFlushTimer = setTimeout(() => {
        stateChangeFlushTimer = null;
        const batch = pendingStateChanges.splice(0);
        if (batch.length === 0) return;
        const state = get();
        const merged = new Set(state.pendingRefreshTargets);
        const batchTargets = new Set<ResourceKey>();
        let maxSeq = state.lastSeq;
        let latestSidebarCounts = state.sidebarCounts;
        let latestChangeOrigin = state.lastChangeOrigin;
        for (const item of batch) {
          if (item.seq <= maxSeq) continue;
          maxSeq = item.seq;
          const nextTargets = planRefreshTargets(item);
          for (const key of nextTargets) {
            merged.add(key);
            batchTargets.add(key);
          }
          if (item.sidebar_counts) latestSidebarCounts = item.sidebar_counts;
          latestChangeOrigin = item.origin;
        }
        if (maxSeq <= state.lastSeq) return;
        set({
          lastSeq: maxSeq,
          pendingRefreshTargets: merged,
          lastPlannedRefreshTargets: batchTargets,
          refreshTargetVersion: state.refreshTargetVersion + 1,
          sidebarCounts: latestSidebarCounts,
          lastChangeOrigin: latestChangeOrigin,
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
      const snapshot: RuntimeSnapshot = await getRuntimeSnapshot();
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
      logBestEffortError('stateChangeStore.refreshSnapshot', error);
    }
  },

  markRefreshTargetHandled: (key) => {
    set((state) => {
      if (!state.pendingRefreshTargets.has(key)) return {};
      const pendingRefreshTargets = new Set(state.pendingRefreshTargets);
      pendingRefreshTargets.delete(key);
      return { pendingRefreshTargets };
    });
  },

  queueRefreshTargets: (keys) => {
    set((state) => {
      const pendingRefreshTargets = new Set(state.pendingRefreshTargets);
      for (const key of keys) pendingRefreshTargets.add(key);
      return { pendingRefreshTargets };
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
