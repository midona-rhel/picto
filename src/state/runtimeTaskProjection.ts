import type { RuntimeTask } from '../shared/types/generated/runtime-contract';
import type { GroupProgressEvent } from '../shared/types/api';

export interface RuntimeTaskProjection {
  groupProgressById: Map<string, GroupProgressEvent>;
}

export function projectRuntimeTasks(
  tasks: Iterable<RuntimeTask>,
): RuntimeTaskProjection {
  const groupProgressById = new Map<string, GroupProgressEvent>();

  for (const task of tasks) {
    if (task.kind !== 'subscription_group') continue;
    if (task.status !== 'running' && task.status !== 'cancelling') continue;

    const groupId = task.task_id.replace(/^group:/, '');
    if (!task.progress) continue;

    groupProgressById.set(groupId, {
      group_id: groupId,
      done: task.progress.done,
      total: task.progress.total,
      remaining: task.progress.total - task.progress.done,
    } as GroupProgressEvent);
  }

  return { groupProgressById };
}
