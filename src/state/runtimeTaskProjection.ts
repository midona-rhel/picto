import type { RuntimeTask } from '../shared/types/generated/runtime-contract';
import type { FlowProgressEvent } from '../features/subscriptions/api';

export interface RuntimeTaskProjection {
  flowProgressById: Map<string, FlowProgressEvent>;
}

export function projectRuntimeTasks(
  tasks: Iterable<RuntimeTask>,
): RuntimeTaskProjection {
  const flowProgressById = new Map<string, FlowProgressEvent>();

  for (const task of tasks) {
    if (task.kind !== 'flow') continue;
    if (task.status !== 'running' && task.status !== 'cancelling') continue;

    const flowId = task.task_id.replace(/^flow:/, '');
    if (!task.progress) continue;

    flowProgressById.set(flowId, {
      flow_id: flowId,
      done: task.progress.done,
      total: task.progress.total,
      remaining: task.progress.total - task.progress.done,
    } as FlowProgressEvent);
  }

  return { flowProgressById };
}
