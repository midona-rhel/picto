import { invoke } from './ipc';
import type { ForegroundTask } from '../shared/types/generated/application/ForegroundTask';
import type { TaskFailure } from '../shared/types/generated/application/TaskFailure';

export function listTasks(): Promise<ForegroundTask[]> {
  return invoke<ForegroundTask[]>('tasks.list');
}

export function pauseTask(taskId: number): Promise<ForegroundTask[]> {
  return invoke<ForegroundTask[]>('tasks.pause', { task_id: taskId });
}

export function resumeTask(taskId: number): Promise<ForegroundTask[]> {
  return invoke<ForegroundTask[]>('tasks.resume', { task_id: taskId });
}

/** Stop a task; with `undo`, also remove what it imported. */
export function stopTask(taskId: number, undo: boolean): Promise<ForegroundTask[]> {
  return invoke<ForegroundTask[]>('tasks.stop', { task_id: taskId, undo });
}

export function dismissTask(taskId: number): Promise<ForegroundTask[]> {
  return invoke<ForegroundTask[]>('tasks.dismiss', { task_id: taskId });
}

export function taskFailures(taskId: number): Promise<TaskFailure[]> {
  return invoke<TaskFailure[]>('tasks.failures', { task_id: taskId });
}
