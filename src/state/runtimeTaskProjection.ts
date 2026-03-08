import type { RuntimeTask } from '../shared/types/generated/runtime-contract';
import type { PtrBootstrapStatus, PtrSyncProgress } from '../shared/controllers/ptrSyncController';
import type { FlowProgressEvent } from '../features/subscriptions/api';

export interface RuntimeTaskProjection {
  runningFlowIds: Set<string>;
  flowProgressById: Map<string, FlowProgressEvent>;
  ptrSyncing: boolean;
  ptrProgress: PtrSyncProgress | null;
  ptrBootstrapStatus: PtrBootstrapStatus | null;
}

export function projectRuntimeTasks(
  tasks: Iterable<RuntimeTask>,
  currentPtrBootstrapStatus: PtrBootstrapStatus | null,
): RuntimeTaskProjection {
  const runningFlowIds = new Set<string>();
  const flowProgressById = new Map<string, FlowProgressEvent>();
  let ptrSyncing = false;
  let ptrProgress: PtrSyncProgress | null = null;
  let ptrBootstrapStatus = currentPtrBootstrapStatus;

  for (const task of tasks) {
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
      continue;
    }

    if (task.kind === 'ptr_sync') {
      if (task.status === 'running' || task.status === 'cancelling') {
        ptrSyncing = true;
        if (task.detail) ptrProgress = task.detail as PtrSyncProgress;
      }
      continue;
    }

    if (task.kind === 'ptr_bootstrap' && task.detail) {
      ptrBootstrapStatus = task.detail as PtrBootstrapStatus;
    }
  }

  return {
    runningFlowIds,
    flowProgressById,
    ptrSyncing,
    ptrProgress,
    ptrBootstrapStatus,
  };
}
