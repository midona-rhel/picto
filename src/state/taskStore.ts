/**
 * Centralized long-running task orchestration.
 *
 * Derives per-family busy/blocking state from the runtime task store.
 * Controllers check `canStart(family)` before launching operations;
 * UI reads `isRunning(family)` and `getProgress(family)` for display.
 */
import { create } from 'zustand';
import { useStateChangeStore } from '../runtime/stateChanges/stateChangeStore';
import type { TaskKind, RuntimeTask } from '../shared/types/backendState';
import type {
  GroupFinishedEvent,
  GroupProgressEvent,
  SubscriptionFinishedEvent,
} from '../shared/types/api';
import { projectRuntimeTasks } from './runtimeTaskProjection';
import { notifyError } from '../shared/lib/notify';
import { logBestEffortError } from '../shared/lib/asyncOps';

// ── Task families ──────────────────────────────────────────────────────────
// Each family maps to one or more backend TaskKind values.

export type TaskFamily =
  | 'import'
  | 'export'
  | 'subscription_run'
  | 'ai_tagger'
  | 'library_switch';

const FAMILY_TO_KINDS: Record<TaskFamily, TaskKind[]> = {
  import: ['import'],
  export: ['export' as TaskKind],
  subscription_run: ['subscription', 'subscription_group'],
  ai_tagger: ['model_download'],
  library_switch: [],
};

// Families that block each other — if any of these are running, the others can't start.
const MUTUALLY_EXCLUSIVE: TaskFamily[][] = [
  ['import', 'library_switch'],
  ['export', 'library_switch'],
];

// ── Store ──────────────────────────────────────────────────────────────────

export interface TaskProgress {
  done: number;
  total: number;
  statusText?: string;
  /** Domain-specific counters (imported/skipped/errors/exported) */
  imported?: number;
  skipped?: number;
  errors?: number;
  exported?: number;
}

export interface TaskFamilyState {
  status: 'idle' | 'running' | 'cancelling' | 'finished' | 'failed';
  running: boolean;
  startedAt: string | null;
  taskKey: string | null;
  progress: TaskProgress | null;
  blockingReason: string | null;
  canStart: boolean;
}

/** Controller-owned progress for families that don't use backend RuntimeTask. */
export interface LocalTaskEntry {
  visible: boolean;
  running: boolean;
  completed: boolean;
  failed: boolean;
  label: string;
  startedAt: string | null;
  progress: TaskProgress | null;
}

const emptyLocalTask: LocalTaskEntry = {
  visible: false, running: false, completed: false, failed: false,
  label: '', startedAt: null, progress: null,
};

interface TaskStoreState {
  // Library switch
  librarySwitching: boolean;
  setLibrarySwitching: (value: boolean) => void;

  // Per-family controller-owned progress
  familyProgress: Record<TaskFamily, LocalTaskEntry>;
  startFamily: (family: TaskFamily, label?: string) => void;
  updateFamilyProgress: (family: TaskFamily, progress: TaskProgress) => void;
  finishFamily: (family: TaskFamily) => void;
  failFamily: (family: TaskFamily) => void;
  clearFamily: (family: TaskFamily) => void;

  // Legacy compat
  exportRunning: boolean;
  setExportRunning: (value: boolean) => void;
}

const initialFamilyProgress: Record<TaskFamily, LocalTaskEntry> = {
  import: { ...emptyLocalTask },
  export: { ...emptyLocalTask },
  subscription_run: { ...emptyLocalTask },
  ai_tagger: { ...emptyLocalTask },
  library_switch: { ...emptyLocalTask },
};

export const useTaskStore = create<TaskStoreState>((set) => ({
  librarySwitching: false,
  setLibrarySwitching: (value) => set({ librarySwitching: value }),

  familyProgress: { ...initialFamilyProgress },

  startFamily: (family, label?: string) => set((state) => ({
    familyProgress: {
      ...state.familyProgress,
      [family]: {
        visible: true, running: true, completed: false, failed: false,
        label: label ?? family, startedAt: new Date().toISOString(), progress: null,
      },
    },
  })),

  updateFamilyProgress: (family, progress) => set((state) => ({
    familyProgress: {
      ...state.familyProgress,
      [family]: { ...state.familyProgress[family], progress },
    },
  })),

  finishFamily: (family) => set((state) => ({
    familyProgress: {
      ...state.familyProgress,
      [family]: {
        ...state.familyProgress[family],
        running: false, completed: true,
      },
    },
  })),

  failFamily: (family) => set((state) => ({
    familyProgress: {
      ...state.familyProgress,
      [family]: {
        ...state.familyProgress[family],
        running: false, failed: true,
      },
    },
  })),

  clearFamily: (family) => set((state) => ({
    familyProgress: {
      ...state.familyProgress,
      [family]: { ...emptyLocalTask },
    },
  })),

  exportRunning: false,
  setExportRunning: (value) => set({ exportRunning: value }),
}));

// ── Selectors (pure functions, no store subscription needed) ───────────────

/** Check if a task family is currently running. */
export function isTaskFamilyRunning(family: TaskFamily): boolean {
  if (family === 'library_switch') {
    return useTaskStore.getState().librarySwitching;
  }
  // Check controller-owned local progress first
  const local = useTaskStore.getState().familyProgress[family];
  if (local?.running) return true;
  // Legacy export compat
  if (family === 'export' && useTaskStore.getState().exportRunning) return true;
  // Check backend RuntimeTask
  const kinds = FAMILY_TO_KINDS[family];
  if (kinds.length === 0) return false;
  const store = useStateChangeStore.getState();
  return kinds.some((kind) => store.isAnyTaskRunning(kind));
}

/** Check if a task family can start (not already running, no blocking conflict). */
export function canStartTaskFamily(family: TaskFamily): { allowed: boolean; reason?: string } {
  // Already running?
  if (isTaskFamilyRunning(family)) {
    return { allowed: false, reason: `${family} is already running` };
  }

  // Check mutual exclusion
  for (const group of MUTUALLY_EXCLUSIVE) {
    if (!group.includes(family)) continue;
    for (const other of group) {
      if (other === family) continue;
      if (isTaskFamilyRunning(other)) {
        return { allowed: false, reason: `Cannot start ${family} while ${other} is running` };
      }
    }
  }

  return { allowed: true };
}

/** Get progress for a task family, if running. */
export function getTaskFamilyProgress(family: TaskFamily): TaskProgress | null {
  // Check controller-owned local progress
  const local = useTaskStore.getState().familyProgress[family];
  if (local?.running && local.progress) return local.progress;
  // Fall back to backend RuntimeTask progress
  const kinds = FAMILY_TO_KINDS[family];
  if (kinds.length === 0) return null;
  const store = useStateChangeStore.getState();
  for (const kind of kinds) {
    const tasks = store.getTasksByKind(kind);
    const running = tasks.find((t) => t.status === 'running');
    if (running?.progress) {
      return {
        done: running.progress.done,
        total: running.progress.total,
        statusText: running.progress.status_text,
      };
    }
  }
  return null;
}

/** Get full state for a task family. */
export function getTaskFamilyState(family: TaskFamily): TaskFamilyState {
  const running = isTaskFamilyRunning(family);
  const { allowed, reason } = canStartTaskFamily(family);
  const progress = running ? getTaskFamilyProgress(family) : null;

  let startedAt: string | null = null;
  let taskKey: string | null = null;
  let status: TaskFamilyState['status'] = 'idle';

  const kinds = FAMILY_TO_KINDS[family];
  if (kinds.length > 0) {
    const store = useStateChangeStore.getState();
    for (const kind of kinds) {
      const tasks = store.getTasksByKind(kind);
      // Prefer a running task, fall back to most recent
      const activeTask = tasks.find((t) => t.status === 'running' || t.status === 'cancelling')
        ?? tasks[tasks.length - 1];
      if (activeTask) {
        startedAt = activeTask.started_at;
        taskKey = activeTask.task_id;
        status = activeTask.status as TaskFamilyState['status'];
        break;
      }
    }
  }

  // Override for locally-tracked families
  if (running && status === 'idle') {
    status = 'running';
  }

  return {
    status,
    running,
    startedAt,
    taskKey,
    progress,
    blockingReason: allowed ? null : (reason ?? null),
    canStart: allowed,
  };
}

// ── React hooks for UI consumption ─────────────────────────────────────────

/** Hook: is a task family running? Re-renders on change. */
export function useIsTaskRunning(family: TaskFamily): boolean {
  const librarySwitching = useTaskStore((s) => s.librarySwitching);
  const exportRunning = useTaskStore((s) => s.exportRunning);
  const familyProgress = useTaskStore((s) => s.familyProgress);
  const tasksById = useStateChangeStore((s) => s.tasksById);

  if (family === 'library_switch') return librarySwitching;
  if (familyProgress[family]?.running) return true;
  if (family === 'export' && exportRunning) return true;

  const kinds = FAMILY_TO_KINDS[family];
  for (const kind of kinds) {
    for (const task of tasksById.values()) {
      if (task.kind === kind && task.status === 'running') return true;
    }
  }
  return false;
}

/** Hook: full task family state. Re-renders on relevant changes. */
export function useTaskFamilyState(family: TaskFamily): TaskFamilyState {
  useTaskStore((s) => s.librarySwitching);
  useTaskStore((s) => s.exportRunning);
  useTaskStore((s) => s.familyProgress);
  useStateChangeStore((s) => s.tasksById);
  return getTaskFamilyState(family);
}

// ═══════════════════════════════════════════════════════════════════════════
// Subscription progress — formerly subscriptionProgressStore.ts
// Now part of the centralized task store.
// ═══════════════════════════════════════════════════════════════════════════

export interface RuntimeSubscriptionProgress {
  subscription_id: string;
  subscription_name: string;
  group_name?: string;
  query_id?: string;
  query_name?: string;
  files_downloaded: number;
  files_skipped: number;
  pages_fetched: number;
  status_text: string;
  phase?: string;
  current_post_id?: string | null;
  current_post_items: number;
  posts_processed: number;
  resume_cursor?: string | null;
  last_error?: string | null;
  status: 'running' | 'finished';
  finished_status?: 'succeeded' | 'failed' | 'cancelled';
  failure_kind?: string | null;
  error?: string | null;
}

export interface SubscriptionProgressState {
  subscriptionProgressById: Map<string, RuntimeSubscriptionProgress>;
  lastSubscriptionFinished: SubscriptionFinishedEvent | null;
  subscriptionEventSeq: number;
  groupProgressById: Map<string, GroupProgressEvent>;
  lastGroupFinished: GroupFinishedEvent | null;
  groupEventSeq: number;
  start: () => void;
  stop: () => void;
  refreshFromBackend: () => Promise<void>;
}

const subFinishedTimers = new Map<string, ReturnType<typeof setTimeout>>();
let subRuntimeUnsub: (() => void) | null = null;

function clearSubTimers() {
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

export const useSubscriptionProgressStore = create<SubscriptionProgressState>((set, get) => ({
  subscriptionProgressById: new Map(),
  lastSubscriptionFinished: null,
  subscriptionEventSeq: 0,
  groupProgressById: new Map(),
  lastGroupFinished: null,
  groupEventSeq: 0,

  start: () => {
    if (subRuntimeUnsub) return;
    let prevTasksById = useStateChangeStore.getState().tasksById;
    subRuntimeUnsub = useStateChangeStore.subscribe((state) => {
      const tasksById = state.tasksById;
      if (tasksById === prevTasksById) return;
      for (const [taskId, task] of tasksById) {
        const prev = prevTasksById.get(taskId);
        if (prev === task) continue;
        applySubTaskChange(set, task);
      }
      const projection = projectRuntimeTasks(tasksById.values());
      set({ groupProgressById: projection.groupProgressById });
      prevTasksById = tasksById;
    });
    void get().refreshFromBackend();
  },

  stop: () => {
    if (subRuntimeUnsub) { subRuntimeUnsub(); subRuntimeUnsub = null; }
    clearSubTimers();
    set({
      subscriptionProgressById: new Map(),
      lastSubscriptionFinished: null,
      subscriptionEventSeq: 0,
      groupProgressById: new Map(),
      lastGroupFinished: null,
      groupEventSeq: 0,
    });
  },

  refreshFromBackend: async () => {
    // Lazy import to avoid circular dependency
    const { subscriptionsController } = await import('../controllers/subscriptionsController');
    try {
      const [runningIds, runningProgress] = await Promise.all([
        subscriptionsController.getRunning(),
        subscriptionsController.getRunningProgress().catch((error) => {
          logBestEffortError('subProgress.refresh', error);
          return [];
        }),
      ]);
      set((state) => {
        const running = new Set<string>([...runningIds, ...runningProgress.map((p) => p.subscription_id)]);
        const byId = new Map(state.subscriptionProgressById);
        for (const [subId, p] of byId.entries()) {
          if (p.status === 'running' && !running.has(subId)) byId.delete(subId);
        }
        for (const p of runningProgress) {
          const existing = byId.get(p.subscription_id);
          byId.set(p.subscription_id, {
            subscription_id: p.subscription_id,
            subscription_name: (p.subscription_name ?? '').trim() || existing?.subscription_name || `Subscription ${p.subscription_id}`,
            query_id: p.query_id, query_name: p.query_name ?? existing?.query_name,
            files_downloaded: p.files_downloaded, files_skipped: p.files_skipped,
            pages_fetched: p.pages_fetched, status_text: p.status_text,
            current_post_items: 0, posts_processed: 0, resume_cursor: null, status: 'running',
          });
        }
        const tasks = useStateChangeStore.getState().tasksById;
        const proj = projectRuntimeTasks(tasks.values());
        return { subscriptionProgressById: byId, groupProgressById: proj.groupProgressById };
      });
    } catch (error) { logBestEffortError('subProgress.refresh', error); }
  },
}));

function applySubTaskChange(
  set: (fn: (state: SubscriptionProgressState) => Partial<SubscriptionProgressState>) => void,
  task: RuntimeTask,
) {
  const isRunning = task.status === 'running' || task.status === 'cancelling';
  const isTerminal = task.status === 'finished' || task.status === 'failed';

  if (task.kind === 'subscription_group') {
    if (isTerminal) {
      set((state) => ({
        groupEventSeq: state.groupEventSeq + 1,
        lastGroupFinished: {
          group_id: task.task_id.replace(/^group:/, ''),
          status: task.status === 'finished' ? 'succeeded' : 'failed',
        } as GroupFinishedEvent,
      }));
    } else {
      set((state) => ({ groupEventSeq: state.groupEventSeq + 1 }));
    }
  }

  if (task.kind === 'subscription') {
    const detail = task.detail as Record<string, unknown> | undefined;
    if (!detail) return;
    const subId = (detail.subscription_id as string) ?? task.task_id.replace(/^sub:/, '');
    const timer = subFinishedTimers.get(subId);
    if (timer && isRunning) { clearTimeout(timer); subFinishedTimers.delete(subId); }

    set((state) => {
      const byId = new Map(state.subscriptionProgressById);
      const existing = byId.get(subId);
      const patch: Partial<SubscriptionProgressState> = { subscriptionEventSeq: state.subscriptionEventSeq + 1 };
      if (isRunning) {
        byId.set(subId, {
          subscription_id: subId,
          subscription_name: ((detail.subscription_name as string) ?? '').trim() || existing?.subscription_name || `Subscription ${subId}`,
          group_name: (detail.group_name as string | undefined) ?? existing?.group_name,
          query_id: detail.query_id as string | undefined,
          query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
          files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          pages_fetched: (detail.pages_fetched as number) ?? 0,
          status_text: (detail.status_text as string) ?? 'Running...',
          phase: (detail.phase as string | undefined) ?? existing?.phase,
          current_post_id: (detail.current_post_id as string | undefined) ?? existing?.current_post_id,
          current_post_items: (detail.current_post_items as number) ?? existing?.current_post_items ?? 0,
          posts_processed: (detail.posts_processed as number) ?? existing?.posts_processed ?? 0,
          resume_cursor: (detail.resume_cursor as string | undefined) ?? existing?.resume_cursor,
          last_error: (detail.last_error as string | undefined) ?? existing?.last_error,
          status: 'running',
        });
      } else if (isTerminal) {
        const fs = (detail.finished_status as string) ?? (task.status === 'finished' ? 'succeeded' : 'failed');
        byId.set(subId, {
          subscription_id: subId,
          subscription_name: ((detail.subscription_name as string) ?? '').trim() || existing?.subscription_name || `Subscription ${subId}`,
          group_name: (detail.group_name as string | undefined) ?? existing?.group_name,
          query_id: detail.query_id as string | undefined,
          query_name: (detail.query_name as string | undefined) ?? existing?.query_name,
          files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          pages_fetched: (detail.pages_fetched as number) ?? existing?.pages_fetched ?? 0,
          status_text: (detail.status_text as string) ?? resolveFinishedSubStatusText({ status: fs as 'succeeded' | 'failed' | 'cancelled', failure_kind: detail.failure_kind as string | undefined } as SubscriptionFinishedEvent),
          phase: (detail.phase as string | undefined) ?? 'finished',
          current_post_id: null, current_post_items: 0,
          posts_processed: (detail.posts_processed as number) ?? existing?.posts_processed ?? 0,
          resume_cursor: (detail.resume_cursor as string | undefined) ?? existing?.resume_cursor,
          last_error: (detail.last_error as string | undefined) ?? (detail.error as string | undefined),
          status: 'finished', finished_status: fs as 'succeeded' | 'failed' | 'cancelled',
          failure_kind: detail.failure_kind as string | undefined,
          error: detail.error as string | undefined,
        });
        patch.lastSubscriptionFinished = {
          subscription_id: subId, subscription_name: (detail.subscription_name as string) ?? '',
          status: fs, files_downloaded: (detail.files_downloaded as number) ?? 0,
          files_skipped: (detail.files_skipped as number) ?? 0,
          failure_kind: detail.failure_kind as string | undefined,
          error: detail.error as string | undefined,
        } as SubscriptionFinishedEvent;
      }
      patch.subscriptionProgressById = byId;
      return patch;
    });

    if (isTerminal) {
      const fk = (detail?.failure_kind as string) ?? '';
      if (fk === 'unauthorized' || fk === 'expired') {
        notifyError(`${(detail?.subscription_name as string) ?? 'Subscription'}: credentials expired or invalid.`, 'Authentication Failed');
      }
    }

    if (isTerminal) {
      const existing = subFinishedTimers.get(subId);
      if (existing) clearTimeout(existing);
      subFinishedTimers.set(subId, setTimeout(() => {
        set((state) => {
          const current = state.subscriptionProgressById.get(subId);
          if (!current || current.status !== 'finished') return {};
          const byId = new Map(state.subscriptionProgressById);
          byId.delete(subId);
          return { subscriptionProgressById: byId };
        });
        subFinishedTimers.delete(subId);
      }, lingerMs(task)));
    }
  }
}

