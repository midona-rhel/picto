/**
 * Foreground tasks (imports) as the renderer shows them.
 *
 * The backend owns every task and its progress (`tasks.list`); this keeps the
 * latest list, refreshes it when tasks change and, while one is working, about
 * once a second so the rate and estimate move. It also owns presentation: which
 * task the foreground dialog shows, and which ones run minimized in the bar.
 */
import { atom, getDefaultStore } from 'jotai';
import { listTasks } from '../platform/taskApi';
import type { ForegroundTask } from '../shared/types/generated/application/ForegroundTask';
import { libraryInvalidation } from './libraryInvalidation';

export const tasksAtom = atom<ForegroundTask[]>([]);

export interface TaskPresentation {
  /** The task shown in the foreground dialog. */
  focusedTaskId: number | null;
  /** Tasks sent to the background; they show in the task bar. */
  minimized: ReadonlySet<number>;
}

export const taskPresentationAtom = atom<TaskPresentation>({ focusedTaskId: null, minimized: new Set<number>() });

const store = getDefaultStore();
const WORKING_POLL_MS = 1000;
const REFRESH_COALESCE_MS = 150;

export function isTaskWorking(task: ForegroundTask): boolean {
  return task.state === 'running' || task.state === 'reverting';
}

export function isTaskOpen(task: ForegroundTask): boolean {
  return task.state === 'running' || task.state === 'paused' || task.state === 'reverting';
}

/** Bring a task to the foreground dialog. */
export function showTask(taskId: number): void {
  const current = store.get(taskPresentationAtom);
  const minimized = new Set(current.minimized);
  minimized.delete(taskId);
  store.set(taskPresentationAtom, { focusedTaskId: taskId, minimized });
}

/** Let a task keep working in the background, in the task bar. */
export function minimizeTask(taskId: number): void {
  const current = store.get(taskPresentationAtom);
  const minimized = new Set(current.minimized);
  minimized.add(taskId);
  store.set(taskPresentationAtom, {
    focusedTaskId: current.focusedTaskId === taskId ? null : current.focusedTaskId,
    minimized,
  });
}

/** Replace the list (from a refresh or a control's answer) and keep focus sensible. */
export function applyTasks(tasks: ForegroundTask[]): void {
  store.set(tasksAtom, tasks);
  const current = store.get(taskPresentationAtom);
  const known = new Set(tasks.map((task) => task.task_id));
  const minimized = new Set([...current.minimized].filter((id) => known.has(id)));
  let focusedTaskId = current.focusedTaskId != null && known.has(current.focusedTaskId)
    ? current.focusedTaskId
    : null;
  // A task that is working and not sent to the background takes the
  // foreground, including one resumed after the app was reopened.
  if (focusedTaskId == null) {
    focusedTaskId = tasks.find((task) => isTaskOpen(task) && !minimized.has(task.task_id))?.task_id ?? null;
  }
  if (
    focusedTaskId !== current.focusedTaskId
    || minimized.size !== current.minimized.size
  ) {
    store.set(taskPresentationAtom, { focusedTaskId, minimized });
  }
}

export function startTaskSettle(): () => void {
  let stopped = false;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let coalesceTimer: ReturnType<typeof setTimeout> | undefined;
  let inFlight = false;
  let again = false;

  const schedulePoll = (tasks: ForegroundTask[]) => {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = tasks.some(isTaskWorking) ? setTimeout(() => { void refresh(); }, WORKING_POLL_MS) : undefined;
  };

  const refresh = async () => {
    if (stopped) return;
    if (inFlight) {
      again = true;
      return;
    }
    inFlight = true;
    try {
      const tasks = await listTasks();
      if (stopped) return;
      applyTasks(tasks);
      schedulePoll(tasks);
    } catch (error) {
      console.error('task refresh failed', error);
    } finally {
      inFlight = false;
      if (again && !stopped) {
        again = false;
        void refresh();
      }
    }
  };

  // Every imported batch announces `tasks`; coalesce bursts into one read.
  const unregister = libraryInvalidation.register('tasks', () => {
    if (coalesceTimer) return;
    coalesceTimer = setTimeout(() => {
      coalesceTimer = undefined;
      void refresh();
    }, REFRESH_COALESCE_MS);
  });
  void refresh();

  return () => {
    stopped = true;
    unregister();
    if (pollTimer) clearTimeout(pollTimer);
    if (coalesceTimer) clearTimeout(coalesceTimer);
  };
}
