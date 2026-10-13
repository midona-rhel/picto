/**
 * What an open task keeps from starting. The backend owns the rule
 * (`blocked_actions` in `core/src/tasks.rs`) and names, on each task, the
 * actions it blocks while it runs, is paused or is undoing. The renderer only
 * reads `blocks`, so a new kind of heavy task needs no change here: its
 * controls ask `useTaskGate` / `taskGate` for their own action.
 */
import { getDefaultStore, useAtomValue } from 'jotai';
import { t } from '../i18n';
import { showErrorNotification } from '../shared/lib/notifications';
import type { ForegroundTask } from '../shared/types/generated/application/ForegroundTask';
import { showTask, tasksAtom } from './taskSettle';

/** Actions a task can block; the backend's `blocked_actions` names them. */
export type TaskAction = 'import';

export interface TaskGate {
  blocked: boolean;
  /** The task in the way, if any. */
  task: ForegroundTask | null;
  /** Why the action is unavailable, for a tooltip; null when it is not. */
  reason: string | null;
}

const OPEN = { blocked: false, task: null, reason: null } satisfies TaskGate;

function gateFor(tasks: readonly ForegroundTask[], action: TaskAction): TaskGate {
  const task = tasks.find((candidate) => candidate.blocks.includes(action)) ?? null;
  if (!task) return OPEN;
  return {
    blocked: true,
    task,
    reason: t('Available when “{value0}” finishes', { value0: task.title }),
  };
}

/** The gate for `action` now, for menus built when they open. */
export function taskGate(action: TaskAction): TaskGate {
  return gateFor(getDefaultStore().get(tasksAtom), action);
}

/** The gate for `action`, following the task list. */
export function useTaskGate(action: TaskAction): TaskGate {
  return gateFor(useAtomValue(tasksAtom), action);
}

function blockedTitle(action: TaskAction): string {
  switch (action) {
    case 'import': return t('An import is already running');
  }
}

/**
 * For a start that bypasses disabled controls (a drop, a paste, a shortcut):
 * when blocked, bring the task in the way forward, say why, and return false.
 */
export function allowTaskAction(action: TaskAction): boolean {
  const gate = taskGate(action);
  if (!gate.blocked || !gate.task) return true;
  showTask(gate.task.task_id);
  showErrorNotification({
    title: blockedTitle(action),
    message: t('Let “{value0}” finish or stop it before starting another.', { value0: gate.task.title }),
  });
  return false;
}
